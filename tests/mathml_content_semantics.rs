//! Content MathML lowering keeps the semantic grouping that presentation
//! MathML cannot show by itself.

use mathema::error::{Config, Strictness};
use mathema::{Format, convert_with, from_mathml_with};

fn latex(body: &str) -> String {
    let source = format!("<math>{body}</math>");
    convert_with(
        &source,
        Format::MathML,
        Format::LaTeX,
        &Config::default().with_strictness(Strictness::Lenient),
    )
    .unwrap()
}

#[test]
fn content_subtraction_keeps_right_associativity_boundary() {
    let output =
        latex("<apply><minus/><ci>a</ci><apply><minus/><ci>b</ci><ci>c</ci></apply></apply>");
    assert!(output.contains("a- \\left( b-c \\right)"), "{output}");
}

#[test]
fn content_negative_power_base_is_visibly_grouped() {
    let output = latex("<apply><power/><cn>-2</cn><cn>2</cn></apply>");
    assert!(output.contains("\\left( -2 \\right)"), "{output}");
    assert!(output.contains("^{2}"), "{output}");
}

#[test]
fn content_sum_used_as_factor_is_fenced() {
    let output = latex(
        "<apply><times/><apply><sum/><bvar><ci>i</ci></bvar>\
         <lowlimit><cn>1</cn></lowlimit><uplimit><ci>n</ci></uplimit><ci>i</ci></apply>\
         <ci>x</ci></apply>",
    );
    assert!(output.contains("\\left( \\sum"), "{output}");
    assert!(output.contains("\\right)"), "{output}");
}

#[test]
fn content_logic_sets_and_relations_keep_nested_boundaries() {
    let logic = latex("<apply><and/><apply><or/><ci>a</ci><ci>b</ci></apply><ci>c</ci></apply>");
    assert!(
        logic.contains("\\left( a\\vee b \\right)\\wedge c"),
        "{logic}"
    );

    let set_difference =
        latex("<apply><setdiff/><ci>A</ci><apply><setdiff/><ci>B</ci><ci>C</ci></apply></apply>");
    assert!(
        set_difference.contains("A\\setminus \\left( B\\setminus C \\right)"),
        "{set_difference}"
    );

    let relation = latex("<apply><eq/><apply><eq/><ci>a</ci><ci>b</ci></apply><ci>c</ci></apply>");
    assert!(relation.contains("\\left( a=b \\right)= c"), "{relation}");
}

#[test]
fn content_derivative_uses_bvar_degree_and_mixed_order() {
    let second =
        latex("<apply><diff/><bvar><ci>x</ci><degree><cn>2</cn></degree></bvar><ci>f</ci></apply>");
    assert!(second.contains("\\frac{d^{2}}"), "{second}");
    assert!(second.contains("x}^{2}"), "{second}");

    let mixed = latex(
        "<apply><partialdiff/><bvar><ci>x</ci></bvar><bvar><ci>y</ci></bvar>\
         <ci>f</ci></apply>",
    );
    assert!(mixed.contains("\\frac{{\\partial}^{2}"), "{mixed}");
    assert!(mixed.contains("\\partial x\\partial y"), "{mixed}");
}

#[test]
fn content_derivative_preserves_large_order_and_reports_missing_bvar() {
    let overflow = latex(
        "<apply><partialdiff/><bvar><ci>x</ci><degree><cn>4294967295</cn></degree></bvar>\
         <bvar><ci>y</ci><degree><cn>1</cn></degree></bvar><ci>f</ci></apply>",
    );
    assert!(overflow.contains("4294967295+ 1"), "{overflow}");

    let missing_bvar = from_mathml_with(
        "<math><apply><diff/><degree><cn>2</cn></degree><ci>f</ci></apply></math>",
        &Config::default().with_strictness(Strictness::Lenient),
    )
    .unwrap();
    assert!(
        missing_bvar
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("degree")
                && diagnostic.message.contains("bound variable"))
    );
}

#[test]
fn content_integral_retains_interval_and_domain_qualifiers() {
    let interval =
        latex("<apply><int/><interval><cn>0</cn><cn>1</cn></interval><ci>f</ci></apply>");
    assert!(interval.contains("\\left[ 0,1 \\right]"), "{interval}");

    let domain = latex(
        "<apply><int/><domainofapplication><ci>D</ci></domainofapplication>\
         <bvar><ci>x</ci></bvar><ci>f</ci></apply>",
    );
    assert!(domain.contains("x\\in D"), "{domain}");

    let half_open = latex(
        "<apply><int/><interval closure=\"open-closed\"><cn>0</cn><cn>1</cn></interval>\
         <ci>f</ci></apply>",
    );
    assert!(half_open.contains("\\left( 0,1 \\right]"), "{half_open}");

    let unsupported = from_mathml_with(
        "<math><apply><int/><interval closure=\"invalid\"><cn>0</cn><cn>1</cn></interval>\
         <ci>f</ci></apply></math>",
        &Config::default().with_strictness(Strictness::Lenient),
    )
    .unwrap();
    assert!(
        unsupported
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("interval closure"))
    );
}
