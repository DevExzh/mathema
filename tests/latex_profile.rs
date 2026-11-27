//! External check for the portable pdfLaTeX output profile.

use mathema::{Config, LatexProfile, from_latex_with, from_unicode_math_with};
use std::fs;
use std::process::Command;

#[test]
fn portable_symbols_keep_their_atom_class_and_report_unmapped_text() {
    use mathema::ast::{MathExpr, MathNode, SymbolKind};
    use mathema::writer::{Writer, latex::LatexWriter};
    let config = Config::default().with_latex_profile(LatexProfile::PdfLaTeX);
    let relation = MathExpr::inline(vec![MathNode::Symbol(SymbolKind::Relation, "𝜶".into())]);
    let latex = LatexWriter::write(&relation, &config).unwrap();
    assert!(latex.contains(r"\mathrel{\boldsymbol{\alpha}}"), "{latex}");

    let text = mathema::from_mathml_with("<math><mtext>中</mtext></math>", &config).unwrap();
    let (output, diagnostics) = text.to_latex_with_diagnostics().unwrap();
    assert!(output.contains('中'));
    assert!(diagnostics.iter().any(|d| d.message.contains("U+4E2D")));
}

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

#[test]
fn portable_profile_compiles_unicode_math_alphabets() {
    if Command::new("pdflatex").arg("--version").output().is_err() {
        return;
    }
    let config = Config::default().with_latex_profile(LatexProfile::PdfLaTeX);
    let directory = tempfile::tempdir().expect("temporary LaTeX directory");
    for input in ["𝔤", "𝖠𝖡𝖢𝖣", "𝚌𝚘𝚍𝚎", "𝐯", "𝕮", "𝜶", "𝝏", "𝜶x", "ℕ⊂ℤ⊂ℚ⊂ℝ"]
    {
        let fragment = from_unicode_math_with(input, &config)
            .unwrap()
            .to_latex_with(&config)
            .unwrap();
        let source = format!(
            r#"\documentclass{{article}}\usepackage{{amsmath,amssymb,xcolor}}\begin{{document}}$ {fragment} $\end{{document}}"#
        );
        let tex = directory.path().join("unicode.tex");
        fs::write(&tex, source).unwrap();
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
            .unwrap();
        assert!(
            output.status.success(),
            "{input:?} -> {fragment:?}\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
