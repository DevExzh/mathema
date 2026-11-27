#![no_main]

//! Exercise the prepared renderer and its bounded output paths.
//!
//! The other render target covers the standalone SVG convenience function.
//! This target keeps the same layout alive while using text SVG, standalone
//! SVG metadata, diagnostics, geometry, and both raster encoders.  Comparing
//! prepared and direct output makes this a functional target rather than a
//! panic-only smoke test.

use libfuzzer_sys::fuzz_target;
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::render::raster::{
    ImageFormat, RasterLimits, RasterOptions, render_to_image_compact_with_diagnostics_and_limits,
    render_to_image_compact_with_limits,
};
use mathema::render::{
    RenderOptions, SvgLimits, SvgMode, SvgOptions, prepare_render_compact,
    render_to_svg_compact_with_limits, render_to_svg_standalone_compact_with_limits,
};

fn assert_diagnostics_equal(
    prepared: &[mathema::error::Diagnostic],
    direct: &[mathema::error::Diagnostic],
) {
    assert_eq!(prepared.len(), direct.len(), "diagnostic count differs");
    for (prepared, direct) in prepared.iter().zip(direct) {
        assert_eq!(prepared.kind, direct.kind, "diagnostic kind differs");
        assert_eq!(
            prepared.message, direct.message,
            "diagnostic message differs"
        );
        assert_eq!(prepared.span, direct.span, "diagnostic span differs");
    }
}

fuzz_target!(|data: &[u8]| {
    // Keep the fuzz case bounded because this target deliberately performs
    // several full render passes for one input.
    if data.len() > 4096 {
        return;
    }
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };

    let config = Config::default();
    let Ok(parsed) = LatexReader::parse_compact(input, &config) else {
        return;
    };

    // Vary display mode and colors while keeping the numeric settings in a
    // small, valid range. Invalid CSS colors are intentional because raster
    // diagnostics must make their black fallback observable.
    let options = RenderOptions {
        font_size_px: 8.0 + f64::from(data.first().copied().unwrap_or(0) % 25),
        display_mode: Some(data.get(1).is_some_and(|byte| byte & 1 != 0)),
        fg_color: if data.get(1).is_some_and(|byte| byte & 2 != 0) {
            "not-a-raster-color".into()
        } else {
            "#204060".into()
        },
        ..RenderOptions::default()
    };
    let Ok(prepared) = prepare_render_compact(&parsed.expr, &options) else {
        return;
    };
    let Ok(geometry) = prepared.geometry(&options) else {
        return;
    };
    assert!(geometry.width_px.is_finite() && geometry.height_px.is_finite());
    assert!(geometry.width_px >= 0.0 && geometry.height_px >= 0.0);

    // A generous limit exercises the bounded emitters while retaining a
    // byte-for-byte comparison with their unprepared compact counterparts.
    let svg_limits = SvgLimits::with_max_bytes(2_000_000);
    let prepared_text = prepared.to_svg_text_with_limits(&options, &svg_limits);
    let direct_text = render_to_svg_compact_with_limits(&parsed.expr, &options, &svg_limits);
    assert_eq!(
        prepared_text.is_ok(),
        direct_text.is_ok(),
        "prepared and direct text SVG disagree on success"
    );
    if let (Ok(prepared_text), Ok(direct_text)) = (prepared_text, direct_text) {
        assert_eq!(prepared_text, direct_text, "prepared text SVG differs");
    }

    let prepared_standalone = prepared.to_svg_with_limits(&options, &svg_limits);
    let direct_standalone =
        render_to_svg_standalone_compact_with_limits(&parsed.expr, &options, &svg_limits);
    assert_eq!(
        prepared_standalone.is_ok(),
        direct_standalone.is_ok(),
        "prepared and direct SVG disagree on success"
    );
    if let (Ok(prepared_standalone), Ok(direct_standalone)) =
        (prepared_standalone, direct_standalone)
    {
        assert_eq!(
            prepared_standalone, direct_standalone,
            "prepared SVG differs"
        );
    }

    // Exercise metadata, structured diagnostics, and the second SVG backend
    // against the same prepared layout.
    let profile = SvgOptions::new(options.clone())
        .with_mode(if data.get(2).is_some_and(|byte| byte & 1 != 0) {
            SvgMode::Text
        } else {
            SvgMode::Standalone
        })
        .with_title("fuzz")
        .with_aria_label("formula");
    if let Ok(profile) = profile.with_id_prefix("fuzz_") {
        let prepared_profile =
            prepared.to_svg_with_svg_options_and_limits_and_diagnostics(&profile, &svg_limits);
        let direct_profile = mathema::render::render_to_svg_compact_with_options_and_limits(
            &parsed.expr,
            &profile,
            &svg_limits,
        );
        assert_eq!(
            prepared_profile.is_ok(),
            direct_profile.is_ok(),
            "prepared and direct profiled SVG disagree on success"
        );
        if let (Ok((prepared_profile, _)), Ok(direct_profile)) = (prepared_profile, direct_profile)
        {
            assert_eq!(prepared_profile, direct_profile, "profiled SVG differs");
        }
    }
    let _ = prepared.to_svg_text_with_diagnostics(&options);
    let _ = prepared.to_svg_with_diagnostics(&options);

    // Exercise both PNG/JPEG output paths and ensure the prepared path shares
    // exactly the same raster bytes as direct compact rendering.
    let raster_options = RasterOptions {
        render: options,
        format: if data.get(2).is_some_and(|byte| byte & 2 != 0) {
            ImageFormat::Jpeg
        } else {
            ImageFormat::Png
        },
        scale: 0.75 + f64::from(data.get(3).copied().unwrap_or(0) % 4) * 0.25,
        background: if data.get(3).is_some_and(|byte| byte & 1 != 0) {
            Some([240, 240, 240, 255])
        } else {
            None
        },
        quality: 70 + data.get(4).copied().unwrap_or(0) % 31,
    };
    // Keep this deterministic and deliberately small so large accepted
    // formulas exercise the same bounded error path on both render routes.
    let limits = RasterLimits::with_max_pixels(1_000_000);
    let direct_image = render_to_image_compact_with_limits(&parsed.expr, &raster_options, &limits);
    let prepared_image = prepared.to_image_with_limits(&raster_options, &limits);
    assert_eq!(
        prepared_image.is_ok(),
        direct_image.is_ok(),
        "prepared and direct raster disagree on success"
    );
    if let (Ok(prepared_image), Ok(direct_image)) = (prepared_image, direct_image) {
        assert_eq!(
            prepared_image, direct_image,
            "prepared raster output differs"
        );
    }

    let direct_with_diagnostics =
        render_to_image_compact_with_diagnostics_and_limits(&parsed.expr, &raster_options, &limits);
    let prepared_with_diagnostics =
        prepared.to_image_with_diagnostics_and_limits(&raster_options, &limits);
    assert_eq!(
        prepared_with_diagnostics.is_ok(),
        direct_with_diagnostics.is_ok(),
        "prepared and direct raster diagnostics disagree on success"
    );
    if let (Ok((prepared_image, prepared_diagnostics)), Ok((direct_image, direct_diagnostics))) =
        (prepared_with_diagnostics, direct_with_diagnostics)
    {
        assert_eq!(
            prepared_image, direct_image,
            "diagnostic raster output differs"
        );
        assert_diagnostics_equal(&prepared_diagnostics, &direct_diagnostics);
    }
});
