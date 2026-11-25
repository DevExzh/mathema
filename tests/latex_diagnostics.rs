//! What lenient reading reports does not depend on where its buffers are.

use mathema::{Config, Strictness, from_latex_with};

fn reported(latex: &str) -> Vec<String> {
    let config = Config::default().with_strictness(Strictness::Lenient);
    let read = from_latex_with(latex, &config).unwrap();
    read.diagnostics()
        .iter()
        .map(|diagnostic| format!("{} {:?}", diagnostic.message, diagnostic.span))
        .collect()
}

/// Commands that are rewritten and read again, in places that are read more
/// than once.
const REWRITTEN: &[&str] = &[
    r"\left(\AXC{x^22}\UIC{\Braket{y^33}}\DisplayProof",
    r"\left(\Braket{x^22}",
    r"\sum_{\Set{\sqrt\over|y^22}}\qty{3}{x_2.5 y^33}}\Set{\sqrt\over|y^22}",
    r"\prod\limits_{\mathrm{\sqrt[\dv{x_2.5 y^33}{x^22}]{\pdv{f}{\sqrt\over}}}}^{\bra{x^{\over}y^22}\right)\Set{a\over b\over c|y^22}} \left(\ket{a\above1 b}",
    r"\dv{x_2.5 y^33}{x^22}\right)\ce{A^22_33 \ce{A^22_33 x^22}} \sqrt[\Set{a\over b\over c|y^22}]{\begin{array}{c}\ket{1\atop2\atop3}} \text{a $1\atop2\atop3$ b}",
    r"\begin{matrix}\text{a $x^{{a\over b\over c}}_{\dv{a\abovewithdelims b}{x^22}}$ b}&\pu{\pdv{f}{a\over b\over c}}\end{matrix} \begin{array}{c}\left(\underset{\Braket{\sqrt\over}}{\dv{\qty{3}{a\above1 b}}{x^22}} \color{red}\pu{x^{\over}y^22}\middle|\text{a $\bra{a\abovewithdelims b}$ b}",
    r"\begin{matrix}\hbox{$\pdv{f}{x^22}$}&\operatorname{{\bra{x^22}}}\end{matrix} \prod\limits_{\dv{a\abovewithdelims b}{x^22}}^{\sqrt[\bra{a\over b\over c}]{\sqrt[\ce{A^22_33 \Set{a\over b\over c|y^22}}]{\pu{\pu{1\atop2\atop3}}}}} \begin{array}{c}\qty{3}{a\above1 b}",
];

#[test]
fn diagnostics_are_the_same_in_every_read_and_thread() {
    let expected: Vec<Vec<String>> = REWRITTEN.iter().map(|latex| reported(latex)).collect();
    std::thread::scope(|scope| {
        for thread in 0..8usize {
            let expected = &expected;
            scope.spawn(move || {
                // Allocations in between move the buffers.
                let mut held: Vec<Vec<u8>> = Vec::new();
                for read in 0..100usize {
                    for (at, latex) in REWRITTEN.iter().enumerate() {
                        held.push(vec![0; (read * 37 + at * 113 + thread * 57) % 700]);
                        if held.len() > 50 {
                            held.swap_remove((read * 7 + at * 3) % 50);
                        }
                        assert_eq!(reported(latex), expected[at], "{latex}");
                    }
                }
            });
        }
    });
}

/// How often a message with `part` is reported.
fn count(latex: &str, part: &str) -> usize {
    let messages = reported(latex);
    messages
        .iter()
        .filter(|message| message.contains(part))
        .count()
}

#[test]
fn a_rewritten_command_reports_once_however_often_it_is_read() {
    let digit = "only the first digit";
    // Once, though the fence without an end is read again.
    assert_eq!(count(r"\left(\Braket{x^22}", digit), 1);
    assert_eq!(count(r"\left(\left(\Braket{\Braket{x^22}}", digit), 1);
    // Each occurrence: in one command, in two that are alike, in nested ones.
    assert_eq!(count(r"\Braket{x^22}", digit), 1);
    assert_eq!(count(r"\Braket{x^22}\Braket{x^22}", digit), 2);
    assert_eq!(count(r"\Braket{x^22 y^22}", digit), 2);
    assert_eq!(count(r"\Set{x^22|\Set{x^22|x^22}}", digit), 3);
    assert_eq!(count(r"x^22\left(\Braket{x^22}x^22", digit), 3);
    assert_eq!(count(r"\dv{x^22 y^33}{t}", digit), 2);
    assert_eq!(count(r"\left(\dv{x^22 y^33}{t}", digit), 2);
    // A proof without an environment is read from a copy.
    assert_eq!(count(r"\AXC{x^22}\UIC{y^33}\DisplayProof", digit), 2);
    assert_eq!(count(r"\left(\AXC{x^22}\UIC{y^33}\DisplayProof", digit), 2);
    assert_eq!(
        count(
            r"\AxiomC{$x^22$}\AXC{\Braket{z^44}}\BIC{y^33}\DisplayProof x^22",
            digit
        ),
        4
    );
    let divides = "ambiguous";
    assert_eq!(
        count(
            r"\AXC{a\over b\over c}\UIC{a\over b\over c}\DisplayProof",
            divides
        ),
        2
    );
    assert_eq!(count(r"\Braket{a\over b\over c}", divides), 1);
    assert_eq!(
        count(r"\Set{a\over b\over c|{a\over b\over c}}", divides),
        2
    );
    assert_eq!(
        count(r"\left(\Set{a\over b\over c|{a\over b\over c}}", divides),
        2
    );
}

/// A color that stopped at a `\middle` is in force after that one only.
#[test]
fn a_color_before_a_middle_delimiter_is_its_fences() {
    let colored = |latex: &str| {
        let read = mathema::from_latex_with(
            latex,
            &Config::default().with_strictness(Strictness::Lenient),
        );
        read.unwrap().to_mathml().unwrap()
    };
    let mathml =
        colored(r"\Braket{\left.\color{red}a\middle|b\right.}\Braket{\left.c\middle|d\right.}");
    assert_eq!(mathml.matches("mathcolor").count(), 2, "{mathml}");
}
