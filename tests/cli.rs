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
