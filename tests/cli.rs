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
