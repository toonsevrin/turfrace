//! Home shell and match-loading presentation.

use super::super::*;

fn spawn_home_arena(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: percent(2),
                top: percent(9),
                width: percent(53),
                height: percent(82),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(percent(42)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(252, 251, 247)),
            BorderColor::all(INK.with_alpha(0.14)),
        ))
        .with_children(|field| {
            for (left, top, width, height, color) in [
                (7.0, 12.0, 34.0, 28.0, palette_color(0)),
                (59.0, 8.0, 29.0, 34.0, palette_color(4)),
                (48.0, 56.0, 39.0, 31.0, palette_color(2)),
                (9.0, 62.0, 28.0, 23.0, palette_color(3)),
            ] {
                field.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(left),
                        top: percent(top),
                        width: percent(width),
                        height: percent(height),
                        border_radius: BorderRadius::all(percent(45)),
                        ..default()
                    },
                    BackgroundColor(color.with_alpha(0.16)),
                ));
            }
            for (index, (color_id, left, top, phase, speed, rx, ry)) in [
                (0, 33.0, 39.0, 0.2, 0.42, 118.0, 74.0),
                (4, 56.0, 46.0, 2.4, 0.35, 102.0, 96.0),
                (2, 45.0, 55.0, 4.2, 0.47, 138.0, 62.0),
            ]
            .into_iter()
            .enumerate()
            {
                let color = palette_color(color_id);
                field
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(left),
                            top: percent(top),
                            width: px(1),
                            height: px(1),
                            ..default()
                        },
                        UiTransform::default(),
                        HomeRacer {
                            phase,
                            speed,
                            radius_x: rx,
                            radius_y: ry,
                        },
                    ))
                    .with_children(|racer| {
                        racer.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                right: px(0),
                                top: px(-2),
                                width: px(54.0 + index as f32 * 8.0),
                                height: px(5),
                                ..default()
                            },
                            BackgroundColor(color.with_alpha(0.28)),
                        ));
                        racer.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(-8),
                                top: px(-8),
                                width: px(16),
                                height: px(16),
                                border: UiRect::all(px(2)),
                                ..default()
                            },
                            BackgroundColor(color),
                            BorderColor::all(INK),
                        ));
                    });
            }
        });
}

pub(in crate::ui) fn spawn_home(mut commands: Commands, theme: Res<UiTheme>) {
    commands
        .spawn((ScreenRoot, screen_node(), BackgroundColor(PAPER)))
        .with_children(|root| {
            spawn_home_arena(root);
            let mut home_panel = panel_node(percent(90));
            home_panel.position_type = PositionType::Absolute;
            home_panel.left = percent(7);
            home_panel.width = percent(39);
            home_panel.max_width = px(460);
            root.spawn((
                home_panel,
                BackgroundColor(Color::NONE),
                BorderColor::all(INK),
            ))
            .with_children(|panel| {
                spawn_title(panel, &theme, "TURFRACE", 62.0);
                spawn_button(panel, &theme, "PLAY", UiAction::State(AppState::Lobby), 0);
                spawn_button(
                    panel,
                    &theme,
                    "LEADERBOARD",
                    UiAction::State(AppState::LocalLeaderboard),
                    1,
                );
                spawn_button(panel, &theme, "SETTINGS", UiAction::Settings, 2);
            });
        });
}

pub(in crate::ui) fn spawn_match_loading(mut commands: Commands, theme: Res<UiTheme>) {
    commands
        .spawn((ScreenRoot, screen_node(), BackgroundColor(PAPER)))
        .with_children(|root| {
            root.spawn((
                panel_node(percent(90)),
                BackgroundColor(Color::NONE),
                BorderColor::all(INK),
            ))
            .with_children(|panel| {
                spawn_title(panel, &theme, "TURFRACE", 56.0);
                panel
                    .spawn((Node {
                        width: px(170),
                        height: px(10),
                        align_self: AlignSelf::Center,
                        column_gap: px(5),
                        ..default()
                    },))
                    .with_children(|meter| {
                        for color_id in [0, 4, 2] {
                            meter.spawn((
                                Node {
                                    flex_grow: 1.0,
                                    height: px(6),
                                    ..default()
                                },
                                BackgroundColor(palette_color(color_id).with_alpha(0.78)),
                            ));
                        }
                    });
                spawn_subtitle(panel, &theme, "BUILDING ARENA");
            });
        });
}
