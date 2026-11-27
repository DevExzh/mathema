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
    let message = error.to_string();
    assert!(
        message.contains("expected a complete numerator argument"),
        "{message}"
    );
    assert!(matches!(&error, Error::LatexParse { .. }), "{error:?}");
}

#[test]
fn strict_trailing_script_names_the_missing_argument_and_span() {
    let error = match from_latex("x^") {
        Ok(_) => panic!("strict trailing script unexpectedly succeeded"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("missing superscript argument"));
    assert!(
        matches!(error, Error::LatexParse { span: Some(span), .. } if span.start <= 1 && span.end >= 2)
    );
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
fn equation_cross_references_are_reported_as_metadata_loss() {
    let parsed = from_latex_with(r"\eqref{eq:x} + \ref{eq:y}", &lenient()).unwrap();
    let messages: Vec<_> = parsed
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains(r"\eqref cross-reference"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains(r"\ref cross-reference"))
    );
    assert!(
        messages
            .iter()
            .all(|message| !message.contains("undefined control sequence"))
    );
    assert_eq!(parsed.to_latex().unwrap(), "+ ");
}

#[test]
fn cases_keep_the_cases_environment_and_row_pitch_on_latex_write() {
    let source = r"\begin{cases}a&b\\c&d\end{cases}";
    let parsed = from_latex(source).unwrap();
    let written = parsed.to_latex().unwrap();
    assert!(written.contains(r"\begin{cases}"), "{written}");
    let mathml = from_latex(&written).unwrap().to_mathml().unwrap();
    assert!(
        mathml.contains("data-mathema-arraystretch=\"120%\""),
        "{mathml}"
    );
    assert_eq!(from_latex(&written).unwrap().to_latex().unwrap(), written);
}

#[test]
fn supported_prescript_and_unicode_math_accents_round_trip_structurally() {
    let prescript = from_latex(r"\prescript{a}{b}{c}").unwrap();
    let mathml = prescript.to_mathml().unwrap();
    assert!(mathml.contains("<mprescripts"), "{mathml}");
    for source in [r"\overbar{x}", r"\ovhook{x}"] {
        let parsed = from_latex(source).unwrap();
        let mathml = parsed.to_mathml().unwrap();
        assert!(mathml.contains("<mover"), "{source}: {mathml}");
        let accent = if source.contains("ovhook") {
            '\u{0309}'
        } else {
            '\u{0305}'
        };
        assert!(mathml.contains(accent), "{mathml}");
        assert!(parsed.to_unicode_math().unwrap().contains(accent));
        let latex = parsed.to_latex().unwrap();
        assert!(from_latex(&latex).is_ok(), "{source} -> {latex}");
    }
}

#[test]
fn multi_atom_prescripts_keep_their_base_group_on_latex_rewrite() {
    for source in [
        r"\prescript{a}{b}{Vc}",
        r"\prescript{a}{b}{V+c}",
        r"\prescript{a}{b}{V_{i}c}",
        r"\prescript{a}{b}{\frac{a}{b}c}",
    ] {
        let original = from_latex(source).unwrap();
        let first = original.to_latex().unwrap();
        let rewritten = from_latex(&first).unwrap();
        assert_eq!(
            rewritten.to_latex().unwrap(),
            first,
            "LaTeX rewrite is not stable for {source:?}: {first:?}"
        );
        assert_eq!(
            rewritten.to_mathml().unwrap(),
            original.to_mathml().unwrap(),
            "prescript base changed for {source:?}: {first:?}"
        );
    }
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
