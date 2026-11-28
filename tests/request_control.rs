use mathema::{
    Config, Error, Format, OutputLimits, RequestControl, convert_with_control,
    convert_with_control_and_limits, from_latex, from_latex_with_control,
};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, atomic::AtomicBool};
use std::thread;

#[test]
fn a_zero_work_budget_stops_each_reader_at_a_checkpoint() {
    let control = RequestControl::with_max_work(0);
    let result = convert_with_control(
        "x + y",
        Format::LaTeX,
        Format::MathML,
        &Config::default(),
        &control,
    );
    assert!(matches!(
        result,
        Err(Error::LimitExceeded {
            kind: mathema::LimitKind::Work,
            limit: 0
        })
    ));
}

#[test]
fn cancellation_is_shared_without_affecting_an_uncontrolled_request() {
    let control = RequestControl::new();
    control.cancel();
    let cancelled = from_latex_with_control("x", &Config::default(), &control);
    assert!(matches!(cancelled, Err(Error::RequestCancelled)));

    let ordinary = mathema::from_latex("x").unwrap().to_mathml().unwrap();
    assert!(ordinary.contains("<mi>x</mi>"));
}

#[test]
fn controlled_builder_can_complete_and_reports_work() {
    let control = RequestControl::with_max_work(100);
    let parsed = from_latex_with_control("x+y", &Config::default(), &control).unwrap();
    assert!(control.work() > 0);
    assert!(parsed.to_mathml().unwrap().contains("<mi>x</mi>"));
}

#[test]
fn compact_normalization_consumes_controlled_work() {
    let parsed = mathema::from_latex("x+y+z").unwrap();
    let mut compact = parsed.compact_expr().clone();
    let control = RequestControl::with_max_work(0);
    let result = control.scope(|| mathema::ast::normalize::normalize_compact_expr(&mut compact));
    assert!(matches!(
        result,
        Err(Error::LimitExceeded {
            kind: mathema::LimitKind::Work,
            limit: 0
        })
    ));
}

#[test]
fn every_reader_has_a_work_checkpoint() {
    let mtef = mathema::from_latex("x").unwrap().to_mtef().unwrap();
    let inputs = [
        (Format::LaTeX, "x+y".to_owned()),
        (Format::MathML, "<math><mi>x</mi></math>".to_owned()),
        (
            Format::Omml,
            r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>x</m:t></m:r></m:oMath>"#.to_owned(),
        ),
        (Format::UnicodeMath, "x+y".to_owned()),
        (Format::Mtef, mtef),
    ];
    for (format, input) in inputs {
        let control = RequestControl::with_max_work(1);
        let result =
            convert_with_control(&input, format, Format::MathML, &Config::default(), &control);
        assert!(
            matches!(
                result,
                Err(Error::LimitExceeded {
                    kind: mathema::LimitKind::Work,
                    ..
                })
            ),
            "{format:?} did not consume more than one work unit: {result:?}"
        );
    }
}

#[test]
fn controlled_text_writers_charge_work_during_emission() {
    let input = "x+".repeat(1_000);
    for target in [
        Format::LaTeX,
        Format::MathML,
        Format::Omml,
        Format::UnicodeMath,
    ] {
        let control = RequestControl::with_max_work(5_000);
        let result =
            convert_with_control(&input, Format::LaTeX, target, &Config::default(), &control);
        assert!(
            matches!(
                result,
                Err(Error::LimitExceeded {
                    kind: mathema::LimitKind::Work,
                    limit: 5_000,
                })
            ),
            "{target:?} completed without charging emission work: {result:?}"
        );
        assert_eq!(control.work(), 5_000, "{target:?}");
    }
}

#[test]
fn controlled_direct_writer_charges_compact_validation() {
    use mathema::writer::Writer;
    use mathema::writer::mathml::MathmlWriter;

    let parsed = mathema::from_latex("x+y+z").unwrap();
    let control = RequestControl::with_max_work(0);
    let result =
        control.scope(|| MathmlWriter::write_compact(parsed.compact_expr(), &Config::default()));
    assert!(matches!(
        result,
        Err(Error::LimitExceeded {
            kind: mathema::LimitKind::Work,
            limit: 0,
        })
    ));
}

#[test]
fn one_shot_control_carries_cancellation_and_output_limits_through_emission() {
    let control = RequestControl::with_max_work(10_000);
    let error = convert_with_control_and_limits(
        r"\frac{a^2+b^2}{c^2+d^2}",
        Format::LaTeX,
        Format::MathML,
        &Config::default(),
        &control,
        &OutputLimits::with_max_bytes(32),
    )
    .unwrap_err();
    assert!(matches!(error, Error::OutputLimit { limit: 32 }));

    let cancelled = RequestControl::new();
    cancelled.cancel();
    let error = convert_with_control_and_limits(
        "x",
        Format::LaTeX,
        Format::Omml,
        &Config::default(),
        &cancelled,
        &OutputLimits::unlimited(),
    )
    .unwrap_err();
    assert!(matches!(error, Error::RequestCancelled));

    let output_cancelled = Arc::new(AtomicBool::new(true));
    let error = convert_with_control_and_limits(
        "x",
        Format::LaTeX,
        Format::MathML,
        &Config::default(),
        &RequestControl::new(),
        &OutputLimits::unlimited().with_cancellation(output_cancelled),
    )
    .unwrap_err();
    assert!(matches!(error, Error::RequestCancelled));

    let error = convert_with_control_and_limits(
        "x+y",
        Format::LaTeX,
        Format::UnicodeMath,
        &Config::default(),
        &RequestControl::new(),
        &OutputLimits::with_max_bytes(1),
    )
    .unwrap_err();
    assert!(matches!(error, Error::OutputLimit { limit: 1 }));

    #[cfg(feature = "render")]
    {
        let error = convert_with_control_and_limits(
            "x",
            Format::LaTeX,
            Format::Svg,
            &Config::default(),
            &RequestControl::new(),
            &OutputLimits::with_max_bytes(1),
        )
        .unwrap_err();
        assert!(matches!(error, Error::OutputLimit { limit: 1 }));
    }
}

#[test]
fn bounded_one_shot_keeps_pre_normalization_loss_diagnostics() {
    let config = Config::default();
    let control = RequestControl::with_max_work(100_000);
    let legacy = mathema::convert_with_config_and_diagnostics(
        r"\color{red}{x}",
        Format::LaTeX,
        Format::Omml,
        &config,
    )
    .unwrap();
    let bounded = mathema::convert_with_config_and_control_and_limits(
        r"\color{red}{x}",
        Format::LaTeX,
        Format::Omml,
        &config,
        &control,
        &OutputLimits::with_max_bytes(100_000),
    )
    .unwrap();
    assert_eq!(bounded.0, legacy.0);
    assert_eq!(
        bounded.1.iter().map(|d| &d.message).collect::<Vec<_>>(),
        legacy.1.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn discarded_omml_diagnostics_do_not_consume_the_final_byte_budget() {
    let config = Config::default();
    let control = RequestControl::with_max_work(100_000);
    let input = r"\tiny x";
    let legacy =
        mathema::convert_with_config_and_diagnostics(input, Format::LaTeX, Format::Omml, &config)
            .unwrap();
    let bounded = mathema::convert_with_config_and_control_and_limits(
        input,
        Format::LaTeX,
        Format::Omml,
        &config,
        &control,
        &OutputLimits::with_max_bytes(legacy.0.len()),
    )
    .unwrap();
    assert_eq!(bounded.0, legacy.0);
    assert_eq!(
        bounded.1.iter().map(|d| &d.message).collect::<Vec<_>>(),
        legacy.1.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );
}

#[test]
fn discarded_unicode_math_diagnostics_do_not_consume_the_final_byte_budget() {
    let config = Config::default();
    let control = RequestControl::with_max_work(100_000);
    let input = r"\tiny x";
    let legacy = mathema::convert_with_config_and_diagnostics(
        input,
        Format::LaTeX,
        Format::UnicodeMath,
        &config,
    )
    .unwrap();
    let bounded = mathema::convert_with_config_and_control_and_limits(
        input,
        Format::LaTeX,
        Format::UnicodeMath,
        &config,
        &control,
        &OutputLimits::with_max_bytes(legacy.0.len()),
    )
    .unwrap();
    assert_eq!(bounded.0, legacy.0);
    assert_eq!(
        bounded.1.iter().map(|d| &d.message).collect::<Vec<_>>(),
        legacy.1.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );
}

#[test]
fn bounded_latex_preserves_pre_normalization_diagnostics_at_exact_budget() {
    let config = Config::default();
    let control = RequestControl::with_max_work(100_000);
    let input = r"\color{red}{x}";
    let legacy =
        mathema::convert_with_config_and_diagnostics(input, Format::LaTeX, Format::LaTeX, &config)
            .unwrap();
    let bounded = mathema::convert_with_config_and_control_and_limits(
        input,
        Format::LaTeX,
        Format::LaTeX,
        &config,
        &control,
        &OutputLimits::with_max_bytes(legacy.0.len()),
    )
    .unwrap();
    assert_eq!(bounded.0, legacy.0);
    assert_eq!(
        bounded.1.iter().map(|d| &d.message).collect::<Vec<_>>(),
        legacy.1.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );
}

#[test]
fn bounded_latex_keeps_dropped_text_diagnostics_before_normalization() {
    let config = Config::default();
    let control = RequestControl::with_max_work(100_000);
    let input = format!(r"\color{{red}}{{\text{{{}}}}}", '\u{13}');
    let legacy =
        mathema::convert_with_config_and_diagnostics(&input, Format::LaTeX, Format::LaTeX, &config)
            .unwrap();
    let bounded = mathema::convert_with_config_and_control_and_limits(
        &input,
        Format::LaTeX,
        Format::LaTeX,
        &config,
        &control,
        &OutputLimits::with_max_bytes(legacy.0.len()),
    )
    .unwrap();
    assert!(
        !legacy.1.is_empty(),
        "the actual writer must report the control character"
    );
    assert_eq!(bounded.0, legacy.0);
    assert_eq!(
        bounded.1.iter().map(|d| &d.message).collect::<Vec<_>>(),
        legacy.1.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );
}

#[test]
fn every_latex_script_budget_short_circuits_without_panicking() {
    use mathema::writer::Writer;
    use mathema::writer::latex::LatexWriter;

    for input in ["x_i^2", "x^2+y^2=z^2", "x^{a_b}", "x_{a^b}^{c_d}"] {
        let parsed = from_latex(input).unwrap();
        let expected =
            LatexWriter::write_compact(parsed.compact_expr(), &Config::default()).unwrap();
        let unlimited = LatexWriter::write_compact_with_limits(
            parsed.compact_expr(),
            &Config::default(),
            &OutputLimits::unlimited(),
        )
        .unwrap();
        assert_eq!(unlimited, expected, "unlimited output differs for {input}");
        let exact = LatexWriter::write_compact_with_limits(
            parsed.compact_expr(),
            &Config::default(),
            &OutputLimits::with_max_bytes(expected.len()),
        )
        .unwrap();
        assert_eq!(exact, expected, "exact output differs for {input}");
        for cap in 0..=expected.len() {
            let result = catch_unwind(AssertUnwindSafe(|| {
                LatexWriter::write_compact_with_limits(
                    parsed.compact_expr(),
                    &Config::default(),
                    &OutputLimits::with_max_bytes(cap),
                )
            }));
            let result = result.unwrap_or_else(|_| panic!("writer panicked for {input} at {cap}"));
            match result {
                Ok(output) => {
                    assert!(output.len() <= cap);
                    assert_eq!(output, expected, "{input} at {cap}");
                },
                Err(Error::OutputLimit { limit }) => assert_eq!(limit, cap),
                Err(error) => panic!("unexpected {error:?} for {input} at {cap}"),
            }
        }
    }
}

#[test]
fn nested_scopes_restore_the_outer_control() {
    let outer = RequestControl::with_max_work(100);
    let inner = RequestControl::with_max_work(0);
    let result = outer.scope(|| {
        assert!(matches!(
            inner.scope(|| convert_with_control(
                "x",
                Format::LaTeX,
                Format::MathML,
                &Config::default(),
                &inner,
            )),
            Err(Error::LimitExceeded {
                kind: mathema::LimitKind::Work,
                ..
            })
        ));
        convert_with_control(
            "x",
            Format::LaTeX,
            Format::MathML,
            &Config::default(),
            &outer,
        )
    });
    assert!(result.is_ok());
}

#[test]
fn controls_are_independent_across_threads_and_panic_scopes_restore_tls() {
    let blocked = RequestControl::with_max_work(0);
    let allowed = RequestControl::with_max_work(100);
    let blocked_thread = thread::spawn(move || {
        convert_with_control(
            "x",
            Format::LaTeX,
            Format::MathML,
            &Config::default(),
            &blocked,
        )
    });
    let allowed_thread = thread::spawn(move || {
        convert_with_control(
            "x",
            Format::LaTeX,
            Format::MathML,
            &Config::default(),
            &allowed,
        )
    });
    assert!(matches!(
        blocked_thread.join().unwrap(),
        Err(Error::LimitExceeded {
            kind: mathema::LimitKind::Work,
            ..
        })
    ));
    assert!(allowed_thread.join().unwrap().is_ok());

    let panicking = RequestControl::with_max_work(0);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _: Result<(), Error> = panicking.scope(|| panic!("scope test"));
    }));
    assert!(result.is_err());
    assert!(mathema::from_latex("x").is_ok());
}

#[test]
fn capped_diagnostics_match_unbounded_recovery_and_surviving_target_losses() {
    let config = Config::default();
    let source = r"\color{red}{\unknown{x}}";
    for target in [Format::UnicodeMath, Format::Omml, Format::LaTeX] {
        let (expected, diagnostics) =
            mathema::convert_with_config_and_diagnostics(source, Format::LaTeX, target, &config)
                .unwrap();
        let (actual, bounded) = mathema::convert_with_config_and_control_and_limits(
            source,
            Format::LaTeX,
            target,
            &config,
            &RequestControl::new(),
            &OutputLimits::with_max_bytes(expected.len()),
        )
        .unwrap();
        assert_eq!(expected, actual);
        assert!(!diagnostics.is_empty());
        let messages = |entries: Vec<mathema::Diagnostic>| {
            entries
                .into_iter()
                .map(|entry| (entry.kind, entry.message, entry.span))
                .collect::<Vec<_>>()
        };
        assert_eq!(messages(diagnostics), messages(bounded), "{target}");
    }
}

#[cfg(feature = "render")]
#[test]
fn bounded_svg_preserves_missing_glyph_and_host_paint_diagnostics() {
    let source = r#"<math><mstyle mathcolor="var(--ink)"><mtext>中</mtext></mstyle></math>"#;
    let (expected, diagnostics) =
        mathema::convert_with_diagnostics(source, Format::MathML, Format::Svg).unwrap();
    let (output, bounded) = mathema::convert_with_config_and_control_and_limits(
        source,
        Format::MathML,
        Format::Svg,
        &Config::default(),
        &RequestControl::new(),
        &OutputLimits::with_max_bytes(expected.len()),
    )
    .unwrap();
    assert_eq!(output, expected);
    assert_eq!(diagnostics.len(), 2);
    assert!(
        diagnostics
            .iter()
            .zip(&bounded)
            .all(|(a, b)| a.kind == b.kind && a.message == b.message)
    );
    assert_eq!(diagnostics.len(), bounded.len());
}

#[test]
fn concurrent_charges_share_one_work_limit() {
    let control = RequestControl::with_max_work(101);
    let start = std::sync::Barrier::new(4);
    let successes = thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    let mut successes = 0;
                    loop {
                        match control.checkpoint(3) {
                            Ok(()) => successes += 1,
                            Err(Error::LimitExceeded {
                                kind: mathema::LimitKind::Work,
                                limit: 101,
                            }) => return successes,
                            Err(error) => panic!("unexpected checkpoint error: {error}"),
                        }
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .sum::<usize>()
    });
    assert_eq!(successes, 33);
    assert_eq!(control.work(), 99);
    control.checkpoint(2).unwrap();
    assert_eq!(control.work(), 101);
}

#[test]
fn shared_work_counter_refuses_overflow_at_the_maximum_budget() {
    let control = RequestControl::with_max_work(usize::MAX);
    control.checkpoint(usize::MAX).unwrap();
    assert!(matches!(
        control.checkpoint(1),
        Err(Error::LimitExceeded {
            kind: mathema::LimitKind::Work,
            ..
        })
    ));
    assert_eq!(control.work(), usize::MAX);
}

#[test]
fn profiled_latex_reports_literal_unicode_even_after_color_normalization() {
    let config = Config::default().with_latex_profile(mathema::LatexProfile::PdfLaTeX);
    let source = r"\textcolor{red}{🙂}";
    let regular =
        mathema::convert_with_config_and_diagnostics(source, Format::LaTeX, Format::LaTeX, &config)
            .unwrap();
    let bounded = mathema::convert_with_config_and_control_and_limits(
        source,
        Format::LaTeX,
        Format::LaTeX,
        &config,
        &RequestControl::new(),
        &OutputLimits::with_max_bytes(regular.0.len()),
    )
    .unwrap();
    assert_eq!(bounded.0, regular.0);
    let messages =
        |list: Vec<mathema::Diagnostic>| list.into_iter().map(|r| r.message).collect::<Vec<_>>();
    assert!(
        !regular.1.is_empty(),
        "literal Unicode needs a portable-profile diagnostic"
    );
    assert_eq!(messages(bounded.1), messages(regular.1));
}

#[cfg(feature = "render")]
#[test]
fn composing_svg_limits_keeps_each_cancellation_source() {
    use std::sync::atomic::Ordering;
    let parsed = mathema::from_latex("x").unwrap();
    for cancel in 0..4 {
        let first = Arc::new(AtomicBool::new(false));
        let second = Arc::new(AtomicBool::new(false));
        let output_request = RequestControl::new();
        let svg_request = RequestControl::new();
        let output = OutputLimits::unlimited()
            .with_cancellation(first.clone())
            .with_request_control(&output_request);
        let limits = mathema::SvgLimits::default()
            .with_cancellation(second.clone())
            .with_output_limits(&output)
            .with_request_control(&svg_request);
        match cancel {
            0 => first.store(true, Ordering::Release),
            1 => second.store(true, Ordering::Release),
            2 => output_request.cancel(),
            _ => svg_request.cancel(),
        }
        assert!(
            matches!(
                parsed.to_svg_with_svg_options_and_limits(&mathema::SvgOptions::default(), &limits),
                Err(Error::RequestCancelled)
            ),
            "cancellation source {cancel}"
        );
    }
}

#[cfg(feature = "render")]
#[test]
fn controlled_svg_conversion_keeps_an_output_limit_request_control() {
    let output_request = RequestControl::new();
    let conversion_request = RequestControl::new();
    let limits = OutputLimits::unlimited().with_request_control(&output_request);
    output_request.cancel();

    let error = convert_with_control_and_limits(
        "x",
        Format::LaTeX,
        Format::Svg,
        &Config::default(),
        &conversion_request,
        &limits,
    )
    .expect_err("the output request cancellation must survive conversion composition");
    assert!(matches!(error, Error::RequestCancelled));
}

#[cfg(feature = "render")]
#[test]
fn controlled_svg_charges_layout_work_for_direct_and_prepared_outputs() {
    let parsed = mathema::from_latex("x + y").unwrap();
    let control = RequestControl::with_max_work(0);
    let direct = control.scope(|| parsed.to_svg());
    assert!(matches!(
        direct,
        Err(Error::LimitExceeded {
            kind: mathema::LimitKind::Work,
            limit: 0
        })
    ));

    let prepared = parsed
        .prepare_render(&mathema::RenderOptions::default())
        .unwrap();
    let control = RequestControl::with_max_work(0);
    let geometry = control.scope(|| prepared.geometry(&mathema::RenderOptions::default()));
    assert!(
        geometry.is_ok(),
        "geometry should not scan the layout under control"
    );

    let control = RequestControl::with_max_work(0);
    let prepared_result =
        control.scope(|| prepared.to_svg_text(&mathema::RenderOptions::default()));
    assert!(matches!(
        prepared_result,
        Err(Error::LimitExceeded {
            kind: mathema::LimitKind::Work,
            limit: 0
        })
    ));
}

#[cfg(feature = "render")]
#[test]
fn bounded_svg_limits_cover_flat_and_nested_prepared_equations() {
    use std::sync::atomic::Ordering;

    for input in ["x + y + z", r#"\frac{a+b}{\sqrt{c^2+d^2}}"#] {
        let parsed = mathema::from_latex(input).unwrap();
        let error = parsed
            .to_svg_with_svg_options_and_limits(
                &mathema::SvgOptions::default(),
                &mathema::SvgLimits::with_max_bytes(1),
            )
            .unwrap_err();
        assert!(matches!(error, Error::OutputLimit { limit: 1 }));

        let cancelled = Arc::new(AtomicBool::new(true));
        let limits = mathema::SvgLimits::default().with_cancellation(cancelled.clone());
        let error = parsed
            .to_svg_with_svg_options_and_limits(&mathema::SvgOptions::default(), &limits)
            .unwrap_err();
        assert!(matches!(error, Error::RequestCancelled));
        cancelled.store(false, Ordering::Release);
    }
}

#[cfg(feature = "render")]
#[test]
fn controlled_one_shot_svg_completes_with_a_generous_budget() {
    let control = RequestControl::with_max_work(100_000);
    let output = convert_with_control_and_limits(
        r#"\frac{a+b}{\sqrt{c^2+d^2}}"#,
        Format::LaTeX,
        Format::Svg,
        &Config::default(),
        &control,
        &OutputLimits::unlimited(),
    )
    .unwrap();
    assert!(output.starts_with("<svg "));
    assert!(control.work() > 0);
}
