//! Canonical competitor identity colors shared by lobby, UI, and 3D presentation.

use bevy::prelude::*;

/// Specification palette in stable color-id order.
pub const PLAYER_COLORS: [Srgba; 12] = [
    Srgba::rgb(
        0x25 as f32 / 255.0,
        0x63 as f32 / 255.0,
        0xeb as f32 / 255.0,
    ),
    Srgba::rgb(
        0xdc as f32 / 255.0,
        0x26 as f32 / 255.0,
        0x26 as f32 / 255.0,
    ),
    Srgba::rgb(
        0x16 as f32 / 255.0,
        0xa3 as f32 / 255.0,
        0x4a as f32 / 255.0,
    ),
    Srgba::rgb(
        0x7c as f32 / 255.0,
        0x3a as f32 / 255.0,
        0xed as f32 / 255.0,
    ),
    Srgba::rgb(
        0xea as f32 / 255.0,
        0x58 as f32 / 255.0,
        0x0c as f32 / 255.0,
    ),
    Srgba::rgb(
        0x08 as f32 / 255.0,
        0x91 as f32 / 255.0,
        0xb2 as f32 / 255.0,
    ),
    Srgba::rgb(
        0xdb as f32 / 255.0,
        0x27 as f32 / 255.0,
        0x77 as f32 / 255.0,
    ),
    Srgba::rgb(
        0xd9 as f32 / 255.0,
        0x77 as f32 / 255.0,
        0x06 as f32 / 255.0,
    ),
    Srgba::rgb(
        0x0f as f32 / 255.0,
        0x76 as f32 / 255.0,
        0x6e as f32 / 255.0,
    ),
    Srgba::rgb(
        0x43 as f32 / 255.0,
        0x38 as f32 / 255.0,
        0xca as f32 / 255.0,
    ),
    Srgba::rgb(
        0x4d as f32 / 255.0,
        0x7c as f32 / 255.0,
        0x0f as f32 / 255.0,
    ),
    Srgba::rgb(
        0x85 as f32 / 255.0,
        0x4d as f32 / 255.0,
        0x0e as f32 / 255.0,
    ),
];

pub fn palette_color(index: u8) -> Color {
    Color::Srgba(PLAYER_COLORS[index as usize % PLAYER_COLORS.len()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_competitor_has_a_stable_color() {
        assert_eq!(PLAYER_COLORS.len(), crate::ids::MAX_COMPETITORS);
        assert_eq!(palette_color(12), palette_color(0));
    }
}
