use mathema::{Config, Error, Strictness, from_latex, from_latex_with};

fn lenient() -> Config {
    Config::default().with_strictness(Strictness::Lenient)
}

#[test]
fn incomplete_fraction_stays_one_literal_command() {
    let parsed = from_latex_with(r"\frac{1", &lenient()).unwrap();
    let mathml = parsed.to_mathml().unwrap();
    assert!(mathml.contains(r"<mtext>\frac{1</mtext>"), "{mathml}");
    let diagnostic = parsed
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.message.contains("incomplete \\frac command"))
        .expect("incomplete-fraction diagnostic");
    assert_eq!(
        diagnostic.span.map(|span| (span.start, span.end)),
        Some((0, 7))
    );
}

#[test]
fn incomplete_fraction_is_still_an_error_in_strict_mode() {
    let error = match from_latex(r"\frac{1") {
        Ok(_) => panic!("strict parsing unexpectedly accepted an incomplete fraction"),
        Err(error) => error,
    };
    assert!(matches!(error, Error::LatexParse { .. }), "{error:?}");
}

#[test]
fn trailing_script_marker_keeps_the_valid_prefix() {
    let parsed = from_latex_with("x^", &lenient()).unwrap();
    assert_eq!(parsed.to_latex().unwrap(), "x");
    let diagnostic = parsed
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.message.contains("missing superscript argument"))
        .expect("missing-script diagnostic");
    assert_eq!(
        diagnostic.span.map(|span| (span.start, span.end)),
        Some((1, 2))
    );
}

#[test]
fn equation_metadata_loss_is_reported() {
    let parsed = from_latex_with(r"\tag{A}\label{eq:x}\notag\nonumber x", &lenient()).unwrap();
    assert_eq!(parsed.to_latex().unwrap(), "x");
    let messages: Vec<_> = parsed
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    assert!(messages.iter().any(|message| message.contains(r"\tag")));
    assert!(messages.iter().any(|message| message.contains(r"\label")));
    assert!(messages.iter().any(|message| message.contains(r"\notag")));
    assert!(
        messages
            .iter()
            .any(|message| message.contains(r"\nonumber"))
    );
}

#[test]
fn incomplete_fraction_recovery_does_not_swallow_node_limits() {
    let config = lenient().with_max_nodes(Some(1));
    let result = from_latex_with(r"\frac{1", &config);
    assert!(matches!(result, Err(Error::LimitExceeded { .. })));
}

#[test]
fn incomplete_fraction_spans_follow_unicode_prefix_and_occurrence() {
    let source = "π+\\frac{1}{2}+\\frac{3";
    let parsed = from_latex_with(source, &lenient()).unwrap();
    let diagnostics: Vec<_> = parsed
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("incomplete \\frac command"))
        .collect();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    let start = source.rfind(r"\frac").unwrap();
    assert_eq!(
        diagnostics[0].span.map(|span| (span.start, span.end)),
        Some((start, source.len()))
    );
}

#[test]
fn macro_expanded_incomplete_fraction_has_no_false_source_span() {
    let parsed = from_latex_with(r"\newcommand{\bad}{\frac{1} }\bad", &lenient()).unwrap();
    let diagnostic = parsed
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.message.contains("incomplete \\frac command"))
        .expect("incomplete-fraction diagnostic");
    assert!(diagnostic.span.is_none());
}
