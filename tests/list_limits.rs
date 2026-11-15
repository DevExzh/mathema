//! A node holds at most `AstArena::MAX_CHILDREN` children. Lists longer than
//! that must be refused with `LimitKind::ListLength` by every reader and by
//! normalization, never truncated.

use mathema::{AstArena, Error, Format, LimitKind, convert};

fn is_list_limit<T>(result: &mathema::Result<T>) -> bool {
    matches!(
        result,
        Err(Error::LimitExceeded {
            kind: LimitKind::ListLength,
            limit: AstArena::MAX_CHILDREN,
        })
    )
}

#[test]
fn omml_rows_flattened_by_normalization_are_refused() {
    // Each m:e holds fewer nodes than the limit; normalizing the delimiter
    // content joins them into one list of 80 001 nodes. (Alternating
    // operators keep adjacent letters from being merged into one node.)
    let runs = "<m:r><m:t>x</m:t></m:r><m:r><m:t>+</m:t></m:r>".repeat(20_000);
    let omml = format!(
        "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">\
         <m:d><m:e>{runs}</m:e><m:e>{runs}</m:e></m:d></m:oMath>"
    );
    let result = convert(&omml, Format::Omml, Format::LaTeX);
    assert!(is_list_limit(&result), "{:?}", result.map(|s| s.len()));
}

#[test]
fn latex_rows_beyond_the_limit_are_refused() {
    let latex = "x".repeat(70_000);
    let result = convert(&latex, Format::LaTeX, Format::MathML);
    assert!(is_list_limit(&result), "{:?}", result.map(|s| s.len()));
}

#[test]
fn mathml_rows_beyond_the_limit_are_refused() {
    let mathml = format!("<math><mrow>{}</mrow></math>", "<mi>x</mi>".repeat(70_000));
    let result = convert(&mathml, Format::MathML, Format::LaTeX);
    assert!(is_list_limit(&result), "{:?}", result.map(|s| s.len()));
}

#[test]
fn unicode_math_rows_beyond_the_limit_are_refused() {
    let input = "x ".repeat(70_000);
    let result = convert(&input, Format::UnicodeMath, Format::LaTeX);
    assert!(is_list_limit(&result), "{:?}", result.map(|s| s.len()));
}

#[test]
fn mtef_lines_beyond_the_limit_are_refused() {
    // MTEF 5: header, FULL, a LINE of 70 000 CHAR records, END, END.
    let mut mtef = vec![
        5, 1, 0, 6, 0, b'D', b'S', b'M', b'T', b'6', 0, 0, 0x0A, 0x01, 0x00,
    ];
    for _ in 0..70_000 {
        mtef.extend_from_slice(&[0x02, 0x00, 0x83, b'x', 0x00]);
    }
    mtef.extend_from_slice(&[0, 0]);
    assert!(is_list_limit(&mathema::from_mtef_bytes(&mtef)));
}

#[test]
fn lists_up_to_the_limit_still_convert() {
    let mathml = format!("<math><mrow>{}</mrow></math>", "<mi>x</mi>".repeat(1_000));
    assert!(convert(&mathml, Format::MathML, Format::LaTeX).is_ok());
}
