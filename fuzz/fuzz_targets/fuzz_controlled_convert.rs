#![no_main]

//! Differential fuzzing for the bounded one-shot text conversion API.
//!
//! A successful ordinary conversion supplies the output and diagnostics that
//! a generously controlled request must reproduce.  The same output length is
//! then used to exercise the exact and just-below byte budgets, while separate
//! requests cover cooperative work exhaustion and cancellation.

use std::sync::{Arc, atomic::AtomicBool};

use libfuzzer_sys::fuzz_target;
use mathema::{
    Config, Diagnostic, Error, Format, LimitKind, OutputLimits, RequestControl, convert,
    convert_with_config_and_control_and_limits, convert_with_control_and_limits,
    convert_with_diagnostics,
};

fn diagnostics_equal(controlled: &[Diagnostic], ordinary: &[Diagnostic]) {
    assert_eq!(controlled.len(), ordinary.len(), "diagnostic count differs");
    for (controlled, ordinary) in controlled.iter().zip(ordinary) {
        assert_eq!(controlled.kind, ordinary.kind, "diagnostic kind differs");
        assert_eq!(
            controlled.message, ordinary.message,
            "diagnostic message differs"
        );
        assert_eq!(controlled.span, ordinary.span, "diagnostic span differs");
    }
}

fn is_output_limit(error: &Error, limit: usize) -> bool {
    matches!(error, Error::OutputLimit { limit: actual } if *actual == limit)
}

fn is_work_limit(error: &Error, limit: usize) -> bool {
    matches!(
        error,
        Error::LimitExceeded {
            kind: LimitKind::Work,
            limit: actual,
        } if *actual == limit
    )
}

/// Exercise one text route after a successful ordinary conversion.
fn exercise_route(input: &str, from: Format, to: Format) -> Option<String> {
    let config = Config::default();
    let ordinary = convert_with_diagnostics(input, from, to).ok()?;
    let ordinary_text = ordinary.0.clone();
    let ordinary_plain = convert(input, from, to).ok()?;
    assert_eq!(
        ordinary_text, ordinary_plain,
        "diagnostic route changed output"
    );

    // The no-ceiling controlled route must preserve both output and losses.
    let generous_control = RequestControl::new();
    let controlled = convert_with_config_and_control_and_limits(
        input,
        from,
        to,
        &config,
        &generous_control,
        &OutputLimits::with_max_bytes(ordinary_text.len().saturating_add(4096)),
    )
    .unwrap_or_else(|error| {
        panic!("generous controlled conversion failed for {from:?}->{to:?}: {error}")
    });
    assert_eq!(controlled.0, ordinary_text, "controlled output differs");
    diagnostics_equal(&controlled.1, &ordinary.1);

    // The exact output size must work for the diagnostic entry point. This is
    // the important boundary for formats such as hex MTEF, where the writer
    // must count the retained representation rather than temporary bytes.
    let exact = convert_with_config_and_control_and_limits(
        input,
        from,
        to,
        &config,
        &RequestControl::new(),
        &OutputLimits::with_max_bytes(ordinary_text.len()),
    )
    .unwrap_or_else(|error| {
        panic!("controlled diagnostics failed at exact limit for {from:?}->{to:?}: {error}")
    });
    assert_eq!(exact.0, ordinary_text, "exact-cap output differs");
    diagnostics_equal(&exact.1, &ordinary.1);

    // A larger cap exercises the plain output entry point and catches a
    // ceiling that is accidentally charged twice.
    let larger = ordinary_text.len().saturating_add(1);
    let larger_plain = convert_with_control_and_limits(
        input,
        from,
        to,
        &config,
        &RequestControl::new(),
        &OutputLimits::with_max_bytes(larger),
    )
    .unwrap_or_else(|error| panic!("controlled plain failed at larger limit: {error}"));
    assert_eq!(
        larger_plain, ordinary_text,
        "larger-cap plain output differs"
    );

    // An empty output has no strictly smaller cap and does not necessarily
    // reach a reader checkpoint, so the work-boundary assertions below apply
    // only to an actual formula output.
    if ordinary_text.is_empty() {
        return Some(ordinary_text);
    }

    // Every cap below the measured output must fail at the typed output limit
    // instead of being silently truncated or replaced by an unrelated error.
    if !ordinary_text.is_empty() {
        for limit in [0, ordinary_text.len() - 1] {
            let controlled = convert_with_control_and_limits(
                input,
                from,
                to,
                &config,
                &RequestControl::new(),
                &OutputLimits::with_max_bytes(limit),
            );
            match controlled {
                Err(error) if is_output_limit(&error, limit) => {},
                Err(error) => {
                    panic!("cap {limit} for {from:?}->{to:?} hid a non-output error: {error:?}")
                },
                Ok(output) => panic!(
                    "cap {limit} for {from:?}->{to:?} unexpectedly succeeded with {} bytes",
                    output.len()
                ),
            }
        }
    }

    // A zero-work request and an already-cancelled request must report their
    // own typed control failures, even when output could also be capped.
    let work_limited = convert_with_control_and_limits(
        input,
        from,
        to,
        &config,
        &RequestControl::with_max_work(0),
        &OutputLimits::with_max_bytes(ordinary_text.len()),
    )
    .expect_err("zero-work request unexpectedly completed");
    assert!(
        is_work_limit(&work_limited, 0),
        "zero-work request returned {work_limited:?}"
    );

    let cancelled = RequestControl::new();
    cancelled.cancel();
    let cancelled_result = convert_with_control_and_limits(
        input,
        from,
        to,
        &config,
        &cancelled,
        &OutputLimits::unlimited(),
    )
    .expect_err("cancelled request unexpectedly completed");
    assert!(
        matches!(cancelled_result, Error::RequestCancelled),
        "cancelled request returned {cancelled_result:?}"
    );

    let output_cancelled = Arc::new(AtomicBool::new(true));
    let output_cancelled_result = convert_with_control_and_limits(
        input,
        from,
        to,
        &config,
        &RequestControl::new(),
        &OutputLimits::unlimited().with_cancellation(output_cancelled),
    )
    .expect_err("output-cancelled request unexpectedly completed");
    assert!(
        matches!(output_cancelled_result, Error::RequestCancelled),
        "output cancellation returned {output_cancelled_result:?}"
    );

    Some(ordinary_text)
}

fuzz_target!(|data: &[u8]| {
    // Every route performs several conversions. Keep the input and generated
    // MTEF replay small enough for a sustained campaign.
    if data.len() > 2048 {
        return;
    }
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    let tag = data.first().copied().unwrap_or(0);
    let target = match tag % 5 {
        0 => Format::LaTeX,
        1 => Format::MathML,
        2 => Format::UnicodeMath,
        3 => Format::Omml,
        _ => Format::Mtef,
    };

    // Invalid ordinary input is intentionally ignored here. Reader errors are
    // covered by the per-format reader targets, while this target compares the
    // service contract only after a normal conversion has a valid reference.
    let Some(mtef_hex) = exercise_route(input, Format::LaTeX, target) else {
        return;
    };

    // A successful hex MTEF write is also a compact, ordinary fixture for the
    // binary reader route. Rotate its destination independently so this target
    // exercises controlled MTEF input as well as controlled MTEF output.
    if target == Format::Mtef {
        let replay_target = match tag.wrapping_add(1) % 4 {
            0 => Format::LaTeX,
            1 => Format::MathML,
            2 => Format::UnicodeMath,
            _ => Format::Omml,
        };
        let _ = exercise_route(&mtef_hex, Format::Mtef, replay_target);
    }
});
