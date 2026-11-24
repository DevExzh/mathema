//! The LaTeX writer spells every script argument as TeX reads it: one token
//! or one brace group after each `^`/`_` (`x^{22}`, never `x^22`, which TeX,
//! KaTeX and MathJax all read as `x^{2}2`).

mod common;

use common::check_script_args;
use mathema::{Format, convert};

fn latex(input: &str) -> String {
    convert(input, Format::LaTeX, Format::LaTeX).unwrap()
}

#[test]
fn checker_flags_multi_token_arguments() {
    for bad in [r"x^22", r"x_2.5", r"\int_22^3 f", r"x^ 2", r"x^\mathrm{d}"] {
        assert!(check_script_args(bad).is_err(), "{bad}");
    }
    for good in [
        r"x^{22}",
        r"x^2+1",
        r"x_a^\alpha",
        r"\_22",
        r"\char`\^22",
        r"{}^{\prime}",
        r"x^2.",
    ] {
        if good == r"x^2." {
            assert!(check_script_args(good).is_err(), "{good}");
        } else {
            assert!(check_script_args(good).is_ok(), "{good}");
        }
    }
}

/// Scripts nested in another construct are spelled as in a row: a numeric
/// argument is braced, so a multi-digit or decimal one stays whole.
#[test]
fn nested_numeric_script_arguments_are_braced() {
    let cases = [
        (r"\frac{x^{22}}{2}", r"\frac{x^{22}}{2}"),
        (r"\left( x^{2.5} \right)", r"\left( x^{2.5} \right)"),
        (r"\sqrt{x^{22}}", r"\sqrt{x^{22}}"),
        (r"\int_{x^{22}}^{2} f", r"\int_{x^{22}}^{2} f"),
        (r"\overbrace{x^{22}}^{3}", r"{\overbrace{x^{22}}}^{3}"),
        (r"\int_{22}^{33} f", r"\int_{22}^{33} f"),
        (r"\sum_{i=10}^{100} i", r"\sum_{i= 10}^{100} i"),
        (r"\frac{a_{12}}{b}", r"\frac{a_{12}}{b}"),
        (r"\sqrt[3]{x_{12}}", r"\sqrt[3]{x_{12}}"),
        (r"{x^{12}}^{2}", r"{x^{12}}^{2}"),
        (r"\binom{x^{12}}{y}", r"\binom{x^{12}}{y}"),
        (r"\boxed{x^{12}}", r"\boxed{x^{12}}"),
        // A single digit is braced too, as it is at the top level.
        (r"\frac{x^2}{2}", r"\frac{x^{2}}{2}"),
        (r"\sum\nolimits_0 x", r"\sum\nolimits_{0} x"),
    ];
    for (input, expected) in cases {
        assert_eq!(latex(input), expected, "{input}");
    }
}

/// A MathML `<mn>` can hold any text; written as a script it is one group.
#[test]
fn mathml_number_script_arguments_are_braced() {
    let cases = [
        (
            "<math><mfrac><msup><mi>x</mi><mn>XII</mn></msup><mn>2</mn></mfrac></math>",
            r"\frac{x^{XII}}{2}",
        ),
        (
            "<math><mfrac><msub><mi>x</mi><mn>1,000</mn></msub><mn>2</mn></mfrac></math>",
            r"\frac{x_{1,000}}{2}",
        ),
        (
            "<math><mrow><munderover><mo>∑</mo><mn>10</mn><mn>20</mn></munderover><mi>k</mi></mrow></math>",
            r"\sum_{10}^{20}{}k",
        ),
    ];
    for (input, expected) in cases {
        let written = convert(input, Format::MathML, Format::LaTeX).unwrap();
        assert_eq!(written, expected, "{input}");
    }
}

/// A number followed by a script without a base after a number it cannot
/// join is written as the scripted number it reads back as.
#[test]
fn number_before_a_script_without_a_base_stays_apart() {
    assert_eq!(latex(r"1.1 1.1{}_{a}"), r"1.1{1.1}_a");
    assert_eq!(latex(r"1.1 1.1{}_{a} {{}_{b}c}"), r"1.1{1.1}_a {{}_{b}c}");
}

/// A script argument behind a color the writer cannot spell is written as
/// the bare number it reads back as, braced on both writes (fuzz artifact
/// `crash-6031b45f…`: `2222_{22222222}` became `2222_22222222`).
#[test]
fn number_behind_an_unwritable_color_stays_braced() {
    for input in [
        r"\frac{a}{x_{\color{#zz}22}}",
        r"\frac{a}{2_{\color{#zz}{22}}}",
    ] {
        let first = latex(input);
        assert!(first.ends_with(r"_{22}}"), "{input}: {first}");
        assert_eq!(latex(&first), first, "{input}");
    }
}

/// An accent glyph without an accent command is written as a character
/// token when it is special to TeX; raw, `^` is a marker with no argument.
#[test]
fn special_accent_glyphs_are_character_tokens() {
    let cases = [
        ("^", r"\overset{\char`\^}{x}"),
        ("_", r"\overset{\char`\_}{x}"),
        ("{", r"\overset{\char`\{}{x}"),
        ("%", r"\overset{\char`\%}{x}"),
    ];
    for (glyph, expected) in cases {
        let input = format!("<math><mover accent='true'><mi>x</mi><mo>{glyph}</mo></mover></math>");
        let written = convert(&input, Format::MathML, Format::LaTeX).unwrap();
        assert_eq!(written, expected, "{glyph}");
        assert!(
            convert(&written, Format::LaTeX, Format::LaTeX).is_ok(),
            "{written}"
        );
    }
}

const LATEX_CORPUS: &[&str] = &[
    r"x^{22}",
    r"x^{2.5}",
    r"x_{10}^{20}",
    r"\frac{x^{22}}{2}",
    r"\left( x^{2.5} \right)",
    r"\sqrt{x^{22}}",
    r"\int_{x^{22}}^{2} f",
    r"\overbrace{x^{22}}^{3}",
    r"\underbrace{x}_{33}",
    r"\overset{22}{x}",
    r"\underset{22}{x}",
    r"\stackrel{22}{=}",
    r"\xrightarrow[22]{33}",
    r"\xleftarrow{x^{22}}",
    r"\sideset{_{22}^{33}}{_{44}^{55}}\sum",
    r"{}_{22}^{33}X",
    r"{}^{22}",
    r"a{}^{22}b",
    r"\hat{x}^{22}",
    r"\hat{x^{22}}",
    r"\vec{x}_{12}",
    r"\lim_{n\to 10} a",
    r"\lim\limits_{22} a",
    r"\max_{22} a",
    r"\operatorname*{argmax}_{22} a",
    r"\mathop{f}_{22}^{33}",
    r"\mathop{\hat f}\limits_{22}^{33}",
    r"\sum\limits_{k=1}^{10} k",
    r"\prod\nolimits_{10}^{20}",
    r"\iint_{22} f",
    r"\oint^{2.5}",
    r"\int\limits_{0.5}^{1.5} x",
    r"\ce{H2O}",
    r"\ce{^{227}_{90}Th}",
    r"\ce{SO4^2-}",
    r"\SI{3e5}{m^2}",
    r"\SI{3}{m^{22}}",
    r"\pu{123 kJ mol-1}",
    r"\text{a}^{22}",
    r"x'^{22}",
    r"x''_{12}",
    r"\color{red}{x^{12}}",
    r"\textcolor{blue}{x_{10}}",
    r"\mathbf{x^{12}}",
    r"\mathrm{d}^{12}x",
    r"\begin{pmatrix} x^{12} & y_{34} \\ 5^{67} & 8_{9.5} \end{pmatrix}",
    r"\begin{cases} x^{12} & x > 0 \\ 0 & x_{10} \end{cases}",
    r"\left. x^{12} \right|_{0}^{10}",
    r"\bigl( x^{12} \bigr)^{34}",
    r"\overline{x^{12}}^{34}",
    r"\underline{x_{12}}_{34}",
    r"\tilde{\tilde{x}^{12}}",
    r"\phantom{x^{12}}",
    r"\cancel{x^{12}}",
    r"\boxed{x^{12}}_{34}",
    r"\frac{1}{2}^{12}",
    r"\binom{n}{k}^{12}",
    r"x^{y^{22}}",
    r"x^{12^{34}}",
    r"x_{12_{34}}",
    r"e^{-10.5 t}",
    r"10^{-12}",
    r"2^{10}3^{20}",
    r"1.1 1.1{}_{a} {{}_{b}c}",
    r"1.1 1.1{}_{a}{}_{b}",
    r"\{^{22}{}^{\prime}",
    r"\intop$_\n$0 01111.1 01.1 01 _$_\nolimits$_'_'ab1",
    r"\sum_{\substack{i=10 \\ j=20}} a_{ij}",
    r"\idotsint_{22}",
    r"\int\cdots\int_{22}",
    r"\middle|^{22}",
    r"\left( x \middle|_{12} y \right)",
    r"\displaystyle x^{12}",
    r"\scriptstyle x_{12}",
    r"\overbrace{a+b}^{10}_{20}",
    r"\underbrace{a+b}_{10}^{20}",
    r"\overleftarrow{AB}^{12}",
    r"\widehat{xy}_{12}",
    r"\dot{x}^{12}_{34}",
    r"\big( ^{22}",
    r"\frac{a}{x_{\color{#zz}22}}",
    r"\sqrt{x^{\color{#zz}22}}",
];

const MATHML_CORPUS: &[&str] = &[
    "<math><msup><mi>x</mi><mn>22</mn></msup></math>",
    "<math><mfrac><msup><mi>x</mi><mn>22</mn></msup><mn>2</mn></mfrac></math>",
    "<math><msqrt><msubsup><mi>x</mi><mn>1.5</mn><mn>22</mn></msubsup></msqrt></math>",
    "<math><mover><mi>x</mi><mn>22</mn></mover></math>",
    "<math><munder><mi>x</mi><mn>22</mn></munder></math>",
    "<math><munderover><mo>∫</mo><mn>10</mn><mn>2.5</mn></munderover></math>",
    "<math><munderover><mo movablelimits='false'>∑</mo><mn>10</mn><mn>20</mn></munderover></math>",
    "<math><mmultiscripts><mi>X</mi><mn>12</mn><mn>34</mn><mprescripts/><mn>56</mn><mn>78</mn></mmultiscripts></math>",
    "<math><mmultiscripts><mi>X</mi><none/><mn>34</mn><mprescripts/><mn>5.6</mn><none/></mmultiscripts></math>",
    "<math><msup><mi>x</mi><mn>XII</mn></msup></math>",
    "<math><msup><mi>x</mi><mi>abc</mi></msup></math>",
    "<math><msup><mi>x</mi><mtext>max</mtext></msup></math>",
    "<math><msup><mi>x</mi><mo>++</mo></msup></math>",
    "<math><msub><mi>x</mi><mn>1,000</mn></msub></math>",
    "<math><msub><mi>log</mi><mn>10</mn></msub><mi>x</mi></math>",
    "<math><msub><mo>lim</mo><mn>10</mn></msub><mi>x</mi></math>",
    "<math><msup><mrow><mo>(</mo><mi>x</mi><mo>)</mo></mrow><mn>22</mn></msup></math>",
    "<math><mrow><mo>(</mo><msup><mi>x</mi><mn>22</mn></msup><mo>)</mo></mrow></math>",
    "<math><mtable><mtr><mtd><msup><mi>x</mi><mn>22</mn></msup></mtd></mtr></mtable></math>",
    "<math><mstyle mathcolor='red'><msup><mi>x</mi><mn>22</mn></msup></mstyle></math>",
    "<math><mover accent='true'><mi>x</mi><mo>^</mo></mover><msup><mi/><mn>22</mn></msup></math>",
    "<math><munder accent='true'><mi>x</mi><mo>_</mo></munder></math>",
];

#[test]
fn written_script_arguments_are_one_tex_token() {
    for input in LATEX_CORPUS {
        let Ok(written) = convert(input, Format::LaTeX, Format::LaTeX) else {
            continue;
        };
        if let Err(violation) = check_script_args(&written) {
            panic!("input `{input}`: {violation}");
        }
    }
    for input in MATHML_CORPUS {
        let written = convert(input, Format::MathML, Format::LaTeX).unwrap();
        if let Err(violation) = check_script_args(&written) {
            panic!("input `{input}`: {violation}");
        }
    }
}
