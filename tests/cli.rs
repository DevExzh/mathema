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
