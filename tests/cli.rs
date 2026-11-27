#![cfg(feature = "cli")]

use assert_cmd::Command;
use predicates::prelude::*;

fn mathema() -> Command {
    Command::cargo_bin("mathema").unwrap()
}

#[test]
fn convert_inline_latex_to_mathml() {
    mathema()
        .args(["convert", "--from", "latex", "--to", "mathml", "x^2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<math"));
}

#[test]
fn convert_mathml_standard_profile_removes_private_attributes() {
    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "mathml",
            "--mathml-profile",
            "standard",
            r"\color{red}{x}",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("<math"))
        .stdout(predicate::str::contains("data-").not());
}

#[test]
fn convert_mathml_round_trip_profile_matches_default_output() {
    let default_output = mathema()
        .args(["convert", "--from", "latex", "--to", "mathml", "x^2"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let explicit_output = mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "mathml",
            "--mathml-profile",
            "round-trip",
            "x^2",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(default_output, explicit_output);
}

#[test]
fn mathml_profile_rejects_an_incompatible_target() {
    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "latex",
            "--mathml-profile",
            "standard",
            "x",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires --to mathml"));
}

#[test]
fn mathml_standard_profile_applies_to_binary_mtef() {
    mathema()
        .args([
            "convert",
            "--mtef-binary",
            "--to",
            "mathml",
            "--mathml-profile",
            "standard",
            "tests/fixtures/mtef/v5-plus-minus.bin",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("<math"))
        .stdout(predicate::str::contains("data-").not());
}

#[test]
fn mathml_standard_profile_applies_to_batch_files() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("out");
    let first = dir.path().join("first.tex");
    let second = dir.path().join("second.tex");
    std::fs::write(&first, "x^2").unwrap();
    std::fs::write(&second, "\\frac{1}{2}").unwrap();
    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "mathml",
            "--mathml-profile",
            "standard",
            "--output",
            output.to_str().unwrap(),
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ])
        .assert()
        .success();
    for name in ["first.mml", "second.mml"] {
        let text = std::fs::read_to_string(output.join(name)).unwrap();
        assert!(!text.contains("data-"), "{name}: {text}");
    }
}

#[test]
fn radix_stays_on_the_whole_hexadecimal_number_through_cli_latex() {
    let source = r#"<math><cn type="integer" base="16">FF</cn></math>"#;
    let latex = mathema()
        .args(["convert", "-f", "mathml", "-t", "latex", source])
        .assert()
        .success();
    let mathml = mathema()
        .args(["convert", "-f", "latex", "-t", "mathml"])
        .write_stdin(latex.get_output().stdout.clone())
        .assert()
        .success();
    let output = std::str::from_utf8(&mathml.get_output().stdout).unwrap();
    let document = roxmltree::Document::parse(output).unwrap();
    let subscript = document.root_element().first_element_child().unwrap();
    assert_eq!(subscript.tag_name().name(), "msub", "{output}");
    assert!(subscript.next_sibling_element().is_none(), "{output}");
    let fields: Vec<String> = subscript
        .children()
        .filter(|n| n.is_element())
        .map(|n| {
            n.descendants()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect()
        })
        .collect();
    assert_eq!(fields, ["FF", "16"]);
}

#[test]
fn convert_stdin_latex_to_mathml() {
    mathema()
        .args(["convert", "--from", "latex", "--to", "mathml"])
        .write_stdin("x^2")
        .assert()
        .success()
        .stdout(predicate::str::contains("<math"));
}

#[test]
fn convert_stdin_latex_preserves_control_space() {
    mathema()
        .args(["convert", "--from", "latex", "--to", "latex"])
        .write_stdin("x\\ ")
        .assert()
        .success()
        .stdout(predicate::str::contains("x\\ "));
}

#[test]
fn convert_reports_warnings_by_default_on_stderr() {
    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "unicodemath",
            r"\color{red}{x}",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("x"))
        .stdout(predicate::str::contains("warning").not())
        .stderr(predicate::str::contains("warning"))
        .stderr(predicate::str::contains("color"));
}

#[test]
fn convert_diagnostics_stay_on_stderr() {
    mathema()
        .args([
            "convert",
            "--diagnostics",
            "--from",
            "latex",
            "--to",
            "unicodemath",
            r"\color{red}{x}",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("x"))
        .stdout(predicate::str::contains("warning").not())
        .stderr(predicate::str::contains("warning"))
        .stderr(predicate::str::contains("color"));
}

#[test]
fn convert_batch_latex_preserves_control_space() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("out");
    let first = dir.path().join("first.tex");
    let second = dir.path().join("second.tex");
    std::fs::write(&first, "x\\ ").unwrap();
    std::fs::write(&second, "y\\ ").unwrap();

    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "latex",
            "--output",
            output.to_str().unwrap(),
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(output.join("first.tex")).unwrap(),
        "x\\ "
    );
    assert_eq!(
        std::fs::read_to_string(output.join("second.tex")).unwrap(),
        "y\\ "
    );
}

#[test]
fn convert_missing_to_flag() {
    mathema()
        .args(["convert", "--from", "latex", "x^2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--to"));
}

#[test]
fn validate_valid_latex() {
    mathema()
        .args(["validate", "--from", "latex", "x^2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid"));
}

#[test]
fn validate_stdin_latex_preserves_control_space() {
    mathema()
        .args(["validate", "--from", "latex"])
        .write_stdin("x\\ ")
        .assert()
        .success()
        .stdout(predicate::str::contains("valid"));
}

#[test]
fn validate_lenient_reports_diagnostics_on_stderr() {
    mathema()
        .args([
            "validate",
            "--lenient",
            "--diagnostics",
            "--from",
            "latex",
            r"\unknowncommand{x}",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid"))
        .stderr(predicate::str::contains("undefined"));
}

#[test]
fn validate_reports_reader_diagnostics_without_same_format_writer_losses() {
    mathema()
        .args([
            "validate",
            "--diagnostics",
            "--from",
            "unicodemath",
            "■(a&b@c&d)",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid"))
        .stderr(predicate::str::contains("centers table columns").not());
}

#[test]
fn validate_batch_latex_preserves_control_space() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.tex");
    let second = dir.path().join("second.tex");
    std::fs::write(&first, "x\\ ").unwrap();
    std::fs::write(&second, "y\\ ").unwrap();

    mathema()
        .args([
            "validate",
            "--from",
            "latex",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn validate_invalid_mathml() {
    mathema()
        .args(["validate", "--from", "mathml", "not xml <<<"])
        .assert()
        .failure();
}

#[test]
fn validate_auto_detect() {
    mathema()
        .args(["validate", "\\frac{1}{2}"])
        .assert()
        .success();
}

#[test]
fn convert_binary_mtef_file() {
    mathema()
        .args([
            "convert",
            "--from",
            "mtef",
            "--mtef-binary",
            "--to",
            "latex",
            "tests/fixtures/mtef/v5-plus-minus.bin",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r"2k\pi\pm \frac{\pi}{2}"));
}

#[test]
fn convert_binary_mtef_stdin_accepts_non_utf8_bytes() {
    let bytes = std::fs::read("tests/fixtures/mtef/v5-plus-minus.bin").unwrap();
    mathema()
        .args([
            "convert",
            "--from",
            "mtef",
            "--mtef-binary",
            "--to",
            "latex",
        ])
        .write_stdin(bytes)
        .assert()
        .success()
        .stdout(predicate::str::contains(r"2k\pi\pm \frac{\pi}{2}"));
}

#[test]
fn binary_mtef_rejects_incompatible_source_format() {
    mathema()
        .args([
            "convert",
            "--mtef-binary",
            "--from",
            "latex",
            "--to",
            "latex",
            "tests/fixtures/mtef/v5-plus-minus.bin",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be combined"));
}

#[test]
fn binary_mtef_implies_source_format() {
    mathema()
        .args([
            "convert",
            "--mtef-binary",
            "--to",
            "latex",
            "tests/fixtures/mtef/v5-plus-minus.bin",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r"2k\pi\pm \frac{\pi}{2}"));
}

#[test]
fn binary_mtef_rejects_inline_input() {
    mathema()
        .args([
            "convert",
            "--from",
            "mtef",
            "--mtef-binary",
            "--to",
            "latex",
            "0011aabb",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("file path or stdin"));
}

#[test]
fn convert_binary_mtef_batch_writes_each_output() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.mtef");
    let second = dir.path().join("second.mtef");
    let output = dir.path().join("out");
    let bytes = std::fs::read("tests/fixtures/mtef/v5-plus-minus.bin").unwrap();
    std::fs::write(&first, &bytes).unwrap();
    std::fs::write(&second, &bytes).unwrap();

    mathema()
        .args([
            "convert",
            "--from",
            "mtef",
            "--mtef-binary",
            "--to",
            "latex",
            "--output",
            output.to_str().unwrap(),
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(
        std::fs::read_to_string(output.join("first.tex"))
            .unwrap()
            .contains(r"2k\pi\pm \frac{\pi}{2}")
    );
    assert!(
        std::fs::read_to_string(output.join("second.tex"))
            .unwrap()
            .contains(r"2k\pi\pm \frac{\pi}{2}")
    );
}

#[test]
fn validate_binary_mtef_file() {
    mathema()
        .args([
            "validate",
            "--from",
            "mtef",
            "--mtef-binary",
            "tests/fixtures/mtef/v5-plus-minus.bin",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid (Mtef)"));
}

#[test]
fn text_file_input_is_bounded_before_parsing() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("large.tex");
    std::fs::write(&input, vec![b'x'; 4 * 1024 * 1024 + 1]).unwrap();

    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "mathml",
            input.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "input size 4194305 bytes exceeds configured limit of 4194304 bytes",
        ));
}

#[test]
fn binary_file_input_is_bounded_before_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("large.mtef");
    std::fs::write(&input, vec![0xff; 4 * 1024 * 1024 + 1]).unwrap();

    mathema()
        .args([
            "convert",
            "--mtef-binary",
            "--to",
            "latex",
            input.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "input size 4194305 bytes exceeds configured limit of 4194304 bytes",
        ));
}

#[test]
fn binary_stdin_is_bounded_before_decoding() {
    mathema()
        .args(["convert", "--mtef-binary", "--to", "latex"])
        .write_stdin(vec![0xff; 4 * 1024 * 1024 + 1])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "input size 4194305 bytes exceeds configured limit of 4194304 bytes",
        ));
}

#[test]
fn omml_dialect_pure_reports_host_color_loss() {
    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "omml",
            "--omml-dialect",
            "pure",
            r"\color{red}{x}",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("<m:oMath"))
        .stdout(predicate::str::contains("<w:color").not())
        .stderr(predicate::str::contains("run color"));
}

#[test]
fn omml_dialect_word_preserves_color_and_size() {
    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "omml",
            "--omml-dialect",
            "word",
            r"\color{red}{x}",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"<w:color w:val="FF0000"/>"#))
        .stderr(predicate::str::contains("warning").not());

    mathema()
        .args([
            "convert",
            "--mtef-binary",
            "--from",
            "mtef",
            "--to",
            "omml",
            "--omml-dialect",
            "word",
            "tests/fixtures/mtef/govdocs-v5-size-points.bin",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"<w:sz w:val="24"/>"#));
}

#[test]
fn omml_dialect_drawing_preserves_color() {
    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "omml",
            "--omml-dialect",
            "drawing",
            r"\color{red}{x}",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"<a:srgbClr val="FF0000"/>"#))
        .stderr(predicate::str::contains("warning").not());
}

#[test]
fn omml_dialect_applies_to_batch_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.tex");
    let second = dir.path().join("second.tex");
    let output = dir.path().join("out");
    std::fs::write(&first, r"\color{red}{x}").unwrap();
    std::fs::write(&second, r"\color{blue}{y}").unwrap();

    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "omml",
            "--omml-dialect",
            "word",
            "--output",
            output.to_str().unwrap(),
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(
        std::fs::read_to_string(output.join("first.omml"))
            .unwrap()
            .contains("<w:color")
    );
    assert!(
        std::fs::read_to_string(output.join("second.omml"))
            .unwrap()
            .contains("<w:color")
    );
}

#[test]
fn omml_dialect_rejects_non_omml_targets() {
    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "mathml",
            "--omml-dialect",
            "word",
            "x",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires --to omml"));
}

#[cfg(feature = "render")]
#[test]
fn render_latex_to_svg() {
    mathema()
        .args(["render", "--from", "latex", "x^2"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("<svg"));
}

#[cfg(feature = "render")]
#[test]
fn render_svg_backend_can_select_text_explicitly() {
    mathema()
        .args(["render", "--from", "latex", "--svg-mode", "text", "x^2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<text"));
}

#[cfg(feature = "render")]
#[test]
fn convert_svg_uses_standalone_outlines_by_default() {
    mathema()
        .args(["convert", "--from", "latex", "--to", "svg", "x^2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<path"))
        .stdout(predicate::str::contains("<use"));
}

#[cfg(feature = "render")]
#[test]
fn convert_svg_can_select_text_backend() {
    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "svg",
            "--svg-mode",
            "text",
            "x^2",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("<text"));
}

#[cfg(feature = "render")]
#[test]
fn render_reports_recovery_on_stderr() {
    mathema()
        .args(["render", "--lenient", "-f", "latex", r"\frac{1"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("<svg"))
        .stderr(predicate::str::contains("<inline>: warning:"));
}

#[cfg(feature = "render")]
#[test]
fn batch_render_names_the_file_that_needed_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("good.tex");
    let incomplete = dir.path().join("incomplete.tex");
    std::fs::write(&good, "x").unwrap();
    std::fs::write(&incomplete, r"\frac{1").unwrap();
    mathema()
        .args(["render", "--lenient"])
        .arg(&good)
        .arg(&incomplete)
        .assert()
        .success()
        .stderr(predicate::str::contains("incomplete.tex: warning:"))
        .stderr(predicate::str::contains("good.tex: warning:").not());
    assert!(incomplete.with_extension("svg").is_file());
}

#[cfg(feature = "render")]
#[test]
fn render_with_font_size() {
    mathema()
        .args(["render", "--from", "latex", "--font-size", "24", "x^2"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("<svg"));
}

#[cfg(feature = "render")]
#[test]
fn render_stdin_latex_preserves_control_space() {
    mathema()
        .args(["render", "--from", "latex", "--format", "svg"])
        .write_stdin("x\\ ")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("<svg"));
}

#[cfg(feature = "render")]
#[test]
fn render_batch_latex_preserves_control_space() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("out");
    let first = dir.path().join("first.tex");
    let second = dir.path().join("second.tex");
    std::fs::write(&first, "x\\ ").unwrap();
    std::fs::write(&second, "y\\ ").unwrap();

    mathema()
        .args([
            "render",
            "--from",
            "latex",
            "--format",
            "svg",
            "--output",
            output.to_str().unwrap(),
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(
        std::fs::read_to_string(output.join("first.svg"))
            .unwrap()
            .starts_with("<svg")
    );
    assert!(
        std::fs::read_to_string(output.join("second.svg"))
            .unwrap()
            .starts_with("<svg")
    );
}

// --- Auto-detection tests ---

#[test]
fn convert_auto_detect_latex() {
    // No --from flag; content sniffing detects LaTeX
    mathema()
        .args(["convert", "--to", "mathml", "\\sqrt{2}"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<math"));
}

#[test]
fn convert_auto_detect_mathml() {
    mathema()
        .args(["convert", "--to", "latex", "<math><mi>x</mi></math>"])
        .assert()
        .success();
}

#[test]
fn convert_auto_detect_fails_ambiguous() {
    // Plain text "hello" can't be detected
    mathema()
        .args(["convert", "--to", "mathml", "hello"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("could not detect"));
}

// --- File I/O tests ---

#[test]
fn convert_file_input() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.tex");
    std::fs::write(&input, "x^2").unwrap();

    mathema()
        .args(["convert", "--to", "mathml", input.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("<math"));
}

#[test]
fn convert_file_output() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("out.mml");

    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "mathml",
            "--output",
            output.to_str().unwrap(),
            "x^2",
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(&output).unwrap();
    assert!(content.contains("<math"));
}

#[test]
fn convert_file_not_found() {
    // Single nonexistent path is treated as inline text (not a file);
    // without --from, auto-detection fails on a filesystem-looking string.
    mathema()
        .args(["convert", "--to", "mathml", "/nonexistent/path/file.tex"])
        .assert()
        .failure();
}

// --- Exit code tests ---

#[test]
fn exit_code_success() {
    mathema()
        .args(["convert", "--from", "latex", "--to", "mathml", "x"])
        .assert()
        .code(0);
}

#[test]
fn exit_code_data_error() {
    mathema()
        .args([
            "convert",
            "--from",
            "mathml",
            "--to",
            "latex",
            "not xml <<<",
        ])
        .assert()
        .code(65); // EX_DATAERR
}

// --- Help text tests ---

#[test]
fn help_flag() {
    mathema()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Convert math formulas"));
}

#[test]
fn convert_help() {
    mathema()
        .args(["convert", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--to"));
}

#[test]
fn validate_help() {
    mathema()
        .args(["validate", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--from"));
}

// --- Edge cases ---

#[test]
fn convert_empty_input() {
    mathema()
        .args(["convert", "--from", "latex", "--to", "mathml", ""])
        .assert()
        .success();
}

#[test]
fn no_subcommand_shows_help() {
    mathema()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn convert_latex_profile_selects_pdf_latex_box_spelling() {
    mathema()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "latex",
            "--latex-profile",
            "pdflatex",
            r"\colorbox{#f00}{x^2}",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r"\colorbox[HTML]{ff0000}"))
        .stdout(predicate::str::contains(r"\ensuremath"));
}

#[test]
fn latex_output_profile_requires_a_latex_target() {
    mathema()
        .args([
            "convert",
            "-f",
            "latex",
            "-t",
            "mathml",
            "--latex-profile",
            "pdflatex",
            "x",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--to latex"));
}
