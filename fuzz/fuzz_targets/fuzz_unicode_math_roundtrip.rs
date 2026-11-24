#![no_main]
//! LaTeX and MathML through UnicodeMath and back keep their structure.
use libfuzzer_sys::fuzz_target;
use mathema::{Format, convert};

#[path = "../support/unicode_math_roundtrip.rs"]
mod support;

fuzz_target!(|data: &[u8]| {
    // Odd first bytes: hand-written UnicodeMath.
    if let Some((first, data)) = data.split_first()
        && first % 2 == 1
    {
        let unicode = support::unicode_math(&mut support::Choices(data));
        if let Some((kind, what)) = support::check(&unicode, Format::UnicodeMath) {
            panic!("{kind} of UnicodeMath {unicode}\n  {what}");
        }
        return;
    }
    let mut choices = support::Choices(data.get(1..).unwrap_or_default());
    let mut latex = String::new();
    support::latex(&mut choices, 3, &mut latex);
    if let Some((kind, what)) = support::check(&latex, Format::LaTeX) {
        panic!("{kind} of LaTeX {latex}\n  {what}");
    }
    let Ok(mathml) = convert(&latex, Format::LaTeX, Format::MathML) else {
        return;
    };
    if let Some((kind, what)) = support::check(&mathml, Format::MathML) {
        panic!("{kind} of MathML {mathml}\n  {what}");
    }
});
