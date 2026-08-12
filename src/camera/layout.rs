use bevy::{camera::Viewport, prelude::*, window::PrimaryWindow};

use super::PlayerCamera;

/// Keep the viewports visually separate without making the divider a gameplay
/// element. The accent rails identify ownership; the gap itself stays neutral.
const VIEWPORT_GAP: u32 = 3;

/// A pixel-space camera rectangle. Coordinates use Bevy's top-left viewport origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportRect {
    pub position: UVec2,
    pub size: UVec2,
}

/// Computes the specification's centered-row landscape layout.
///
/// `gap` is left as neutral canvas between viewports, producing the divider without
/// another render pass. Slots are returned in visual reading order.
pub fn viewport_layout(count: usize, canvas: UVec2, gap: u32) -> Vec<ViewportRect> {
    if count == 0 || canvas.x == 0 || canvas.y == 0 {
        return Vec::new();
    }
    let count = count.min(8);
    let (columns, rows) = match count {
        1 => (1, 1),
        2 => (2, 1),
        3 | 4 => (2, 2),
        5 | 6 => (3, 2),
        _ => (3, 3),
    };
    let row_counts: Vec<usize> = (0..rows)
        .map(|row| (count - row * columns).min(columns))
        .collect();

    let cell_width = canvas.x as f32 / columns as f32;
    let cell_height = canvas.y as f32 / rows as f32;
    let mut output = Vec::with_capacity(count);
    for (row, &items) in row_counts.iter().enumerate() {
        let row_offset = (columns - items) as f32 * cell_width * 0.5;
        for column in 0..items {
            let x0 = (row_offset + column as f32 * cell_width).round() as u32;
            let x1 = (row_offset + (column + 1) as f32 * cell_width).round() as u32;
            let y0 = (row as f32 * cell_height).round() as u32;
            let y1 = ((row + 1) as f32 * cell_height).round() as u32;

            let left_gap = u32::from(column > 0) * gap.div_ceil(2);
            let right_gap = u32::from(column + 1 < items) * (gap / 2);
            let top_gap = u32::from(row > 0) * gap.div_ceil(2);
            let bottom_gap = u32::from(row + 1 < rows) * (gap / 2);
            output.push(ViewportRect {
                position: UVec2::new(x0 + left_gap, y0 + top_gap),
                size: UVec2::new(
                    x1.saturating_sub(x0 + left_gap + right_gap).max(1),
                    y1.saturating_sub(y0 + top_gap + bottom_gap).max(1),
                ),
            });
        }
    }
    output
}

pub(super) fn update_camera_viewports(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&PlayerCamera, &mut Camera)>,
) {
    let Ok(window) = windows.single() else { return };
    let count = cameras.iter().count();
    let layout = viewport_layout(count, window.physical_size(), VIEWPORT_GAP);
    for (player_camera, mut camera) in &mut cameras {
        if let Some(rect) = layout.get(player_camera.slot as usize) {
            let next = Viewport {
                physical_position: rect.position,
                physical_size: rect.size,
                ..default()
            };
            let changed = camera.viewport.as_ref().is_none_or(|old| {
                old.physical_position != next.physical_position
                    || old.physical_size != next.physical_size
            });
            if changed {
                camera.viewport = Some(next);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_player_uses_the_full_canvas_without_a_split() {
        let layout = viewport_layout(1, UVec2::new(1280, 720), 3);

        assert_eq!(
            layout,
            vec![ViewportRect {
                position: UVec2::ZERO,
                size: UVec2::new(1280, 720),
            }]
        );
    }

    #[test]
    fn three_players_center_the_last_viewport() {
        let layout = viewport_layout(3, UVec2::new(1920, 1080), 2);
        assert_eq!(layout.len(), 3);
        assert_eq!(layout[0].position, UVec2::ZERO);
        assert_eq!(layout[1].position.x, 961);
        assert_eq!(layout[2].position.x, 480);
        assert_eq!(layout[2].size.x, 960);
    }

    #[test]
    fn five_players_center_the_two_player_bottom_row() {
        let layout = viewport_layout(5, UVec2::new(1920, 1080), 2);
        assert_eq!(layout[3].position.x, 320);
        assert_eq!(layout[4].position.x, 961);
        assert_eq!(layout[3].size.x, 639);
    }

    #[test]
    fn eight_players_center_the_last_pair() {
        let layout = viewport_layout(8, UVec2::new(1920, 1080), 2);
        assert_eq!(layout[6].position.x, 320);
        assert_eq!(layout[7].position.x, 961);
        assert!(layout.iter().all(|rect| rect.size.x > 0 && rect.size.y > 0));
    }

    #[test]
    fn layout_caps_at_eight_and_handles_empty_canvas() {
        assert_eq!(viewport_layout(20, UVec2::new(800, 600), 2).len(), 8);
        assert!(viewport_layout(2, UVec2::ZERO, 2).is_empty());
    }
}
