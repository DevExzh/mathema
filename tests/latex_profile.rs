//! External check for the portable pdfLaTeX output profile.

use mathema::{Config, LatexProfile, from_latex_with};
use std::fs;
use std::process::Command;

#[test]
fn portable_colored_math_compiles_and_reimports() {
    let probe = Command::new("pdflatex").arg("--version").output();
    if probe.is_err() {
        return;
    }

    let config = Config::default().with_latex_profile(LatexProfile::PdfLaTeX);
    let directory = tempfile::tempdir().expect("temporary LaTeX directory");
    for input in [
        r"\colorbox{red}{x^2+\frac{a}{b}}",
        r"\fcolorbox{red}{#ff0000}{x^2+\frac{a}{b}}",
        r"\fcolorbox{#ff0000}{green}{x^2}",
        r"\colorbox{red}{\fcolorbox{#f00}{blue}{x^2+\frac{a}{b}}}",
        r"\fcolorbox{#f00}{green}{\colorbox{blue}{\frac{a_1}{b^2}}}",
    ] {
        let result = from_latex_with(input, &config).expect(input);
        let latex = result.to_latex_with(&config).expect(input);
        let reread = from_latex_with(&latex, &config)
            .expect("portable profile output should be readable")
            .to_latex_with(&config)
            .expect("portable profile output should be stable");
        assert_eq!(reread, latex, "{input}");

        let source = format!(
            "\\documentclass{{article}}\n\\usepackage{{amsmath,amssymb,xcolor}}\n\\begin{{document}}\n$ {latex} $\n\\end{{document}}\n"
        );
        let tex = directory.path().join("profile.tex");
        fs::write(&tex, source).expect("write LaTeX profile fixture");
        let output = Command::new("pdflatex")
            .args([
                "-interaction=nonstopmode",
                "-halt-on-error",
                "-no-shell-escape",
                "-output-directory",
            ])
            .arg(directory.path())
            .arg(&tex)
            .output()
            .expect("run pdflatex");
        assert!(
            output.status.success(),
            "pdflatex rejected {latex:?}\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
