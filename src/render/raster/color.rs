//! Raster conversion for the shared, dependency-free paint classifier.

use super::super::paint::{self, PaintClass, Srgba};
use tiny_skia::Color;

pub(super) fn parse_color(value: &str) -> Color {
    resolve_color(value, Color::BLACK)
}

pub(super) fn resolve_color(value: &str, current: Color) -> Color {
    match paint::classify(value) {
        PaintClass::Concrete(Srgba {
            red,
            green,
            blue,
            alpha,
        }) => Color::from_rgba8(red, green, blue, alpha),
        PaintClass::CurrentColor => current,
        _ => Color::BLACK,
    }
}

pub(super) fn parse_color_checked(value: &str) -> Option<Color> {
    match paint::classify(value) {
        PaintClass::Concrete(Srgba {
            red,
            green,
            blue,
            alpha,
        }) => Some(Color::from_rgba8(red, green, blue, alpha)),
        PaintClass::CurrentColor => Some(Color::BLACK),
        _ => None,
    }
}
