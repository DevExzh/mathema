use mathema::error::CompactParseResult;
use mathema::{
    Config, Diagnostic, DiagnosticCode, DiagnosticKind, DiagnosticPhase, Format, MathExpr,
    Strictness, convert_with_diagnostic_records, from_latex_with,
};

#[test]
fn one_shot_records_preserve_read_then_write_phases() {
    let config = Config::default().with_strictness(Strictness::Lenient);
    let (_, records) = mathema::convert_with_config_and_diagnostic_records(
        r"\color{red}{x^}",
        Format::LaTeX,
        Format::UnicodeMath,
        &config,
    )
    .unwrap();

    assert!(
        !records.is_empty(),
        "expected recovery and target-loss records"
    );
    let first_write = records
        .iter()
        .position(|record| record.phase == DiagnosticPhase::Write)
        .expect("target loss phase");
    assert!(
        records[..first_write]
            .iter()
            .all(|record| record.phase == DiagnosticPhase::Read)
    );
    assert!(
        records[first_write..]
            .iter()
            .any(|record| record.code == DiagnosticCode::TargetLoss)
    );
    assert!(
        records[..first_write]
            .iter()
            .any(|record| record.code == DiagnosticCode::ReaderRecovery)
    );
    assert_eq!(records[0].phase.as_str(), "read");
    assert_eq!(records[first_write].code.as_str(), "target_loss");
}

#[test]
fn builder_records_combine_reader_and_requested_target() {
    let config = Config::default().with_strictness(Strictness::Lenient);
    let parsed = from_latex_with(r"\color{red}{x^}", &config).unwrap();
    let records = parsed.diagnostics_for(Format::UnicodeMath).unwrap();

    assert!(records.iter().any(|record| {
        record.phase == DiagnosticPhase::Read
            && record.code == DiagnosticCode::ReaderRecovery
            && record.span.is_some()
    }));
    assert!(records.iter().any(|record| {
        record.phase == DiagnosticPhase::Write && record.code == DiagnosticCode::TargetLoss
    }));
    assert_eq!(
        parsed.diagnostic_records().len(),
        parsed.diagnostics().len()
    );
}

#[test]
fn default_one_shot_record_api_matches_text_output() {
    let (text, records) =
        convert_with_diagnostic_records("x + y", Format::LaTeX, Format::MathML).unwrap();
    assert!(text.contains("<math"));
    assert!(records.is_empty(), "lossless conversion: {records:?}");
}

#[test]
fn valid_mathml_with_approximated_style_is_interpretation_not_recovery() {
    let parsed =
        mathema::from_mathml(r#"<math><mstyle mathsize="300%"><mi>x</mi></mstyle></math>"#)
            .unwrap();
    let records = parsed.diagnostic_records();
    assert!(records.iter().any(|record| {
        record.phase == DiagnosticPhase::Read && record.code == DiagnosticCode::ReaderInterpretation
    }));
    assert!(!records.iter().any(|record| {
        record.phase == DiagnosticPhase::Read && record.code == DiagnosticCode::ReaderRecovery
    }));
}

#[test]
fn omml_interpretation_and_recovery_use_separate_reader_codes() {
    let interpreted = mathema::from_omml(
        r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:nary><m:naryPr><m:grow m:val="1"/></m:naryPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:nary></m:oMath>"#,
    )
    .unwrap();
    let records = interpreted.diagnostic_records();
    assert!(records.iter().any(|record| {
        record.phase == DiagnosticPhase::Read
            && record.code == DiagnosticCode::ReaderInterpretation
            && record.message.contains("growing n-ary")
    }));
    assert!(
        !records
            .iter()
            .any(|record| record.code == DiagnosticCode::ReaderRecovery)
    );

    let lenient = Config::default().with_strictness(Strictness::Lenient);
    let recovered = mathema::from_omml_with(
        r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:notAnOmmlElement/></m:oMath>"#,
        &lenient,
    )
    .unwrap();
    let records = recovered.diagnostic_records();
    assert!(records.iter().any(|record| {
        record.phase == DiagnosticPhase::Read
            && record.code == DiagnosticCode::ReaderRecovery
            && record.message.contains("unsupported element")
    }));
}

#[test]
fn undefined_latex_commands_have_a_distinct_stable_code() {
    let parsed = mathema::from_latex_with(
        r"\foo{x}",
        &Config::default().with_strictness(Strictness::Lenient),
    )
    .unwrap();
    let records = parsed.diagnostic_records();
    assert!(records.iter().any(|record| {
        record.phase == DiagnosticPhase::Read
            && record.code == DiagnosticCode::UndefinedCommand
            && record.span.is_some()
    }));
}

#[test]
fn latex_interpretation_warnings_are_distinct_from_recovery() {
    let config = Config::default().with_strictness(Strictness::Lenient);
    let source = r"\tag{A}\label{eq:x}\eqref{eq:x}\htmlId{node}{x}\num{1^}";
    let parsed = from_latex_with(source, &config).unwrap();
    let records = parsed.diagnostic_records();
    assert!(
        records.iter().any(|record| {
            record.code == DiagnosticCode::ReaderInterpretation
                && record.message.contains(r"\tag equation metadata")
        }),
        "{records:?}"
    );
    assert!(
        records.iter().any(|record| {
            record.code == DiagnosticCode::ReaderInterpretation
                && record.message.contains(r"\eqref cross-reference")
        }),
        "{records:?}"
    );
    assert!(
        records.iter().any(|record| {
            record.code == DiagnosticCode::ReaderInterpretation
                && record.message.contains("does not parse")
        }),
        "{records:?}"
    );
    assert!(
        records
            .iter()
            .all(|record| record.code != DiagnosticCode::ReaderRecovery),
        "well-formed interpretation/loss input was classified as recovery: {records:?}"
    );
}

#[test]
fn latex_missing_arguments_are_reader_recovery() {
    let config = Config::default().with_strictness(Strictness::Lenient);
    for source in ["x^", r"\frac{1"] {
        let parsed = from_latex_with(source, &config).unwrap();
        let records = parsed.diagnostic_records();
        assert!(
            records
                .iter()
                .any(|record| record.code == DiagnosticCode::ReaderRecovery),
            "{source}: {records:?}"
        );
        assert!(
            records
                .iter()
                .all(|record| record.code != DiagnosticCode::ReaderInterpretation),
            "{source}: {records:?}"
        );
    }
}

#[test]
fn latex_table_and_spacing_approximations_are_interpretations() {
    let config = Config::default().with_strictness(Strictness::Lenient);
    for source in [
        r"\begin{unknown}a&b\end{unknown}",
        r"\begin{array}{cc}a&b\\\cline{1-1}c&d\end{array}",
        r"a\hskip 1em plus .2em b",
    ] {
        let parsed = from_latex_with(source, &config).unwrap();
        let records = parsed.diagnostic_records();
        assert!(
            records
                .iter()
                .any(|record| record.code == DiagnosticCode::ReaderInterpretation),
            "{source}: {records:?}"
        );
        assert!(
            records
                .iter()
                .all(|record| record.code != DiagnosticCode::ReaderRecovery),
            "{source}: {records:?}"
        );
    }
}

#[cfg(feature = "render")]
#[test]
fn svg_fallback_records_have_a_glyph_specific_code() {
    let parsed = mathema::from_latex("中").unwrap();
    let records = parsed.diagnostics_for(Format::Svg).unwrap();
    assert!(records.iter().any(|record| {
        record.phase == DiagnosticPhase::Write && record.code == DiagnosticCode::RenderMissingGlyph
    }));
}

#[cfg(feature = "raster")]
#[test]
fn raster_diagnostic_queries_classify_unsupported_colors_without_encoding() {
    let parsed = mathema::from_mathml(
        r#"<math><mstyle mathcolor="var(--math-color)"><mi>x</mi></mstyle></math>"#,
    )
    .unwrap();
    let records = parsed.diagnostics_for(Format::Png).unwrap();
    assert!(records.iter().any(|record| {
        record.phase == DiagnosticPhase::Write && record.code == DiagnosticCode::RenderColorFallback
    }));
}

#[test]
fn legacy_diagnostic_edits_are_reflected_in_new_records() {
    let mut parsed = CompactParseResult::new(
        mathema::CompactExpr::from_math_expr(&MathExpr::inline(vec![])).unwrap(),
    );
    parsed.diagnostics.push(Diagnostic {
        kind: DiagnosticKind::Warning,
        message: "edited diagnostic".into(),
        span: None,
    });
    let records = parsed.diagnostic_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].message, "edited diagnostic");
}

#[test]
fn reordering_legacy_diagnostics_does_not_reuse_an_old_code_slot() {
    use mathema::reader::{Reader, latex::LatexReader};
    let mut compact = LatexReader::parse_compact(r"\mysterysymbol", &Config::default()).unwrap();
    assert_eq!(
        compact.diagnostic_records()[0].code,
        DiagnosticCode::UndefinedCommand
    );
    compact.diagnostics.insert(
        0,
        Diagnostic {
            kind: DiagnosticKind::Warning,
            message: "newly inserted warning".into(),
            span: None,
        },
    );
    let records = compact.diagnostic_records();
    assert_eq!(records[0].code, DiagnosticCode::ReaderRecovery);
    assert_eq!(records[0].message, "newly inserted warning");
    assert_eq!(records[1].code, DiagnosticCode::UndefinedCommand);
    compact.diagnostics[1].message = "different warning".into();
    assert_eq!(
        compact.diagnostic_records()[1].code,
        DiagnosticCode::ReaderRecovery
    );
}

#[test]
fn located_conversion_keeps_latex_utf8_span_and_legacy_error() {
    let error =
        mathema::convert_with_location(r"\frac{é", Format::LaTeX, Format::MathML).unwrap_err();
    assert!(matches!(error.error, mathema::Error::LatexParse { .. }));
    let span = error.span.expect("LaTeX exposes its parser span");
    assert!(span.start <= span.end);
    assert_eq!(error.format, Some(Format::LaTeX));
}

#[test]
fn located_mathml_error_accounts_for_leading_whitespace_and_utf8() {
    let source = "  <math><mi>π</math>  ";
    let error = match mathema::from_mathml_with_location(source, &Config::default()) {
        Ok(_) => panic!("malformed MathML unexpectedly parsed"),
        Err(error) => error,
    };
    assert!(matches!(error.error, mathema::Error::MathmlParse { .. }));
    let span = error.span.expect("quick-xml reports an event position");
    assert!(span.start >= 2 && span.end <= source.len());
    assert!(source.is_char_boundary(span.start) && source.is_char_boundary(span.end));
    assert_eq!(error.format, Some(Format::MathML));
}

#[test]
fn located_mtef_failure_reports_the_binary_wire_offset() {
    let bytes = mathema::from_latex("x")
        .unwrap()
        .to_mtef_bytes(&mathema::MtefWriteOptions::default())
        .unwrap();
    let truncated = &bytes[..bytes.len().min(6)];
    let error = match mathema::from_mtef_bytes_with_location(truncated, &Config::default()) {
        Ok(_) => panic!("truncated MTEF unexpectedly parsed"),
        Err(error) => error,
    };
    assert!(matches!(error.error, mathema::Error::MtefParse { .. }));
    let span = error.span.expect("decoder error_at has a wire offset");
    assert!(span.start <= truncated.len());
    assert_eq!(error.format, Some(Format::Mtef));
    let short = [0_u8];
    let short_error = match mathema::from_mtef_bytes_with_location(&short, &Config::default()) {
        Ok(_) => panic!("short MTEF unexpectedly parsed"),
        Err(error) => error,
    };
    assert!(short_error.span.is_none_or(|span| span.end <= short.len()));
}

#[test]
fn located_omml_failure_reports_an_xml_event_offset() {
    let source = concat!(
        "\u{FEFF}",
        r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r></m:oMath>"#
    );
    let error = match mathema::from_omml_with_location(source, &Config::default()) {
        Ok(_) => panic!("malformed OMML unexpectedly parsed"),
        Err(error) => error,
    };
    assert!(matches!(error.error, mathema::Error::OmmlParse { .. }));
    let span = error.span.expect("XML reader reports an event position");
    assert!(span.start >= 3 && span.end <= source.len());
    assert!(source.is_char_boundary(span.start) && source.is_char_boundary(span.end));
    assert_eq!(error.format, Some(Format::Omml));
}

#[test]
fn mtef_positional_loss_is_interpretation_not_recovery() {
    let mut body = vec![0x0A, 0x01, 0x08, 129, 127, 0x02, 0, 131, b'x', 0, 0, 0];
    let mut bytes = vec![5, 1, 0, 6, 0];
    bytes.extend_from_slice(b"DSMT6\0");
    bytes.push(0);
    bytes.append(&mut body);
    let parsed = mathema::from_mtef_bytes(&bytes).unwrap();
    let records = parsed.diagnostic_records();
    assert!(records.iter().any(|record| {
        record.kind == mathema::DiagnosticKind::Info
            && record.code == DiagnosticCode::ReaderInterpretation
            && record.message.contains("nudge")
            && record.span.is_some_and(|span| span.end <= bytes.len())
    }));
    assert!(!records.iter().any(|record| {
        record.message.contains("nudge") && record.code == DiagnosticCode::ReaderRecovery
    }));
}

#[test]
fn malformed_mtef_recovery_stays_recovery() {
    let bytes = mathema::from_latex("x")
        .unwrap()
        .to_mtef_bytes(&Default::default())
        .unwrap();
    let recovered = mathema::from_mtef_bytes_with(
        &bytes[..bytes.len().saturating_sub(1)],
        &Config::default().with_strictness(Strictness::Lenient),
    )
    .unwrap();
    assert!(recovered.diagnostic_records().iter().any(|record| {
        record.kind == mathema::DiagnosticKind::Warning
            && record.code == DiagnosticCode::ReaderRecovery
            && record
                .span
                .is_some_and(|span| span.end <= bytes.len().saturating_sub(1))
    }));

    let hex: String = bytes[..bytes.len().saturating_sub(1)]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let recovered_hex = mathema::from_mtef_with(
        &hex,
        &Config::default().with_strictness(Strictness::Lenient),
    )
    .unwrap();
    assert!(recovered_hex.diagnostic_records().iter().any(|record| {
        record.code == DiagnosticCode::ReaderRecovery
            && record
                .span
                .is_some_and(|span| span.end <= bytes.len().saturating_sub(1))
    }));
}

#[test]
fn one_shot_location_matches_each_native_reader() {
    let mathml = "<math><mi></math>";
    let direct = match mathema::from_mathml_with_location(mathml, &Config::default()) {
        Ok(_) => panic!("malformed MathML unexpectedly parsed"),
        Err(error) => error,
    };
    let converted =
        mathema::convert_with_location(mathml, Format::MathML, Format::LaTeX).unwrap_err();
    assert_eq!(converted.span, direct.span);
    assert_eq!(converted.format, direct.format);

    let omml = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r></m:oMath>"#;
    let direct = match mathema::from_omml_with_location(omml, &Config::default()) {
        Ok(_) => panic!("malformed OMML unexpectedly parsed"),
        Err(error) => error,
    };
    let converted = mathema::convert_with_location(omml, Format::Omml, Format::LaTeX).unwrap_err();
    assert_eq!(converted.span, direct.span);
    assert_eq!(converted.format, direct.format);

    let bytes = mathema::from_latex("x")
        .unwrap()
        .to_mtef_bytes(&mathema::MtefWriteOptions::default())
        .unwrap();
    let truncated = &bytes[..bytes.len().min(6)];
    let hex: String = truncated.iter().map(|byte| format!("{byte:02x}")).collect();
    let direct = match mathema::from_mtef_bytes_with_location(truncated, &Config::default()) {
        Ok(_) => panic!("truncated MTEF unexpectedly parsed"),
        Err(error) => error,
    };
    let converted = mathema::convert_with_location(&hex, Format::Mtef, Format::LaTeX).unwrap_err();
    assert_eq!(converted.span, direct.span);
    assert_eq!(converted.format, direct.format);
}

#[test]
fn xml_errors_identify_the_original_event_after_multibyte_text() {
    for (format, source, bad_tag) in [
        (
            Format::MathML,
            "  <math><mi>λ</mi><!-- ok --></wrong> ",
            "</wrong>",
        ),
        (
            Format::Omml,
            "﻿<m:oMath xmlns:m='http://schemas.openxmlformats.org/officeDocument/2006/math'><m:r><m:t>λ</m:t></m:r><!-- ok --></wrong>",
            "</wrong>",
        ),
        (
            Format::MathML,
            "  <math><mi>λ</mi></math><math/> ",
            "<math/>",
        ),
    ] {
        let error = mathema::convert_with_location(source, format, Format::LaTeX).unwrap_err();
        let start = source.rfind(bad_tag).unwrap();
        assert_eq!(
            error.span,
            Some(mathema::error::Span::new(start, start + 1)),
            "{format}: {error}"
        );
    }
}

#[test]
fn writer_failure_does_not_report_a_reader_location() {
    let error =
        mathema::convert_with_location("🙂", Format::UnicodeMath, Format::Mtef).unwrap_err();
    assert!(error.span.is_none());
    assert!(error.format.is_none());
}

#[test]
fn located_read_limits_keep_the_input_format_without_inventing_a_position() {
    let config = Config::default().with_max_input_bytes(Some(0));
    for (format, input) in [
        (Format::LaTeX, "x"),
        (Format::MathML, "<math/>"),
        (Format::Omml, "<m:oMath/>"),
        (Format::UnicodeMath, "λ"),
        (Format::Mtef, "05"),
    ] {
        let error =
            mathema::convert_with_config_and_location(input, format, Format::LaTeX, &config)
                .unwrap_err();
        assert!(matches!(error.error, mathema::Error::InputTooLarge { .. }));
        assert_eq!(error.format, Some(format));
        assert_eq!(error.span, None);
    }
}

#[test]
fn undefined_command_text_fallback_reports_its_output_loss_separately() {
    let config = Config::default().with_source_ranges(true);
    let source = r"x+\unmodeledcmd{y}";
    let parsed = from_latex_with(source, &config).unwrap();
    let (output, records) = mathema::convert_with_config_and_diagnostic_records(
        source,
        Format::LaTeX,
        Format::UnicodeMath,
        &config,
    )
    .unwrap();
    assert_eq!(output, parsed.to_unicode_math().unwrap());
    assert_eq!(
        records,
        parsed.diagnostics_for(Format::UnicodeMath).unwrap()
    );
    assert_eq!(
        records
            .iter()
            .map(|r| (r.phase, r.code))
            .collect::<Vec<_>>(),
        [
            (DiagnosticPhase::Read, DiagnosticCode::UndefinedCommand),
            (DiagnosticPhase::Write, DiagnosticCode::TargetLoss),
        ]
    );
    let (_, writer) = parsed.to_unicode_math_with_diagnostics().unwrap();
    assert_eq!(writer[0].message, records[1].message);
}
