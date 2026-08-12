//! Device registration cards and match-composition controls.

use super::super::*;
use crate::lobby::MAX_HUMANS;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::window::PrimaryWindow;

fn card_customization_actions(device: InputDeviceId) -> [LobbyCommand; 6] {
    [
        LobbyCommand::CycleProfile(device, -1),
        LobbyCommand::CycleProfile(device, 1),
        LobbyCommand::CycleColor(device, -1),
        LobbyCommand::CycleColor(device, 1),
        LobbyCommand::CyclePattern(device, -1),
        LobbyCommand::CyclePattern(device, 1),
    ]
}

fn spawn_icon_stepper(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    center: impl Bundle,
    previous: LobbyCommand,
    next: LobbyCommand,
    order: u16,
) {
    parent
        .spawn((Node {
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: px(5),
            ..default()
        },))
        .with_children(|row| {
            spawn_mini_button(row, theme, "<", UiAction::Lobby(previous), order);
            row.spawn(center);
            spawn_mini_button(row, theme, ">", UiAction::Lobby(next), order + 1);
        });
}

fn spawn_device_icon(parent: &mut ChildSpawnerCommands, device: InputDeviceId, color: Color) {
    let (width, height, radius) = match device {
        InputDeviceId::Mouse => (22.0, 30.0, 9.0),
        InputDeviceId::KeyboardPrimary => (38.0, 24.0, 2.0),
        InputDeviceId::Gamepad(_) => (38.0, 24.0, 10.0),
    };
    parent
        .spawn((
            Node {
                width: px(width),
                height: px(height),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(radius)),
                position_type: PositionType::Relative,
                ..default()
            },
            BorderColor::all(color),
        ))
        .with_children(|icon| match device {
            InputDeviceId::Mouse => {
                icon.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(8),
                        top: px(3),
                        width: px(2),
                        height: px(8),
                        ..default()
                    },
                    BackgroundColor(color),
                ));
            }
            InputDeviceId::KeyboardPrimary => {
                for top in [5.0, 12.0] {
                    icon.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(5),
                            top: px(top),
                            width: px(24),
                            height: px(2),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                }
            }
            InputDeviceId::Gamepad(_) => {
                icon.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(7),
                        top: px(6),
                        width: px(3),
                        height: px(9),
                        ..default()
                    },
                    BackgroundColor(color),
                ));
                icon.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(4),
                        top: px(9),
                        width: px(9),
                        height: px(3),
                        ..default()
                    },
                    BackgroundColor(color),
                ));
                for left in [24.0, 29.0] {
                    icon.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(left),
                            top: px(9),
                            width: px(3),
                            height: px(3),
                            border_radius: BorderRadius::all(percent(50)),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                }
            }
        });
}

fn spawn_robot_icon(parent: &mut ChildSpawnerCommands, color: Color) {
    parent
        .spawn((Node {
            width: px(30),
            height: px(28),
            position_type: PositionType::Relative,
            ..default()
        },))
        .with_children(|robot| {
            robot.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(5),
                    top: px(7),
                    width: px(20),
                    height: px(16),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BorderColor::all(color),
            ));
            robot.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(14),
                    top: px(1),
                    width: px(2),
                    height: px(7),
                    ..default()
                },
                BackgroundColor(color),
            ));
            for left in [10.0, 18.0] {
                robot.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(left),
                        top: px(12),
                        width: px(3),
                        height: px(3),
                        border_radius: BorderRadius::all(percent(50)),
                        ..default()
                    },
                    BackgroundColor(color),
                ));
            }
        });
}

pub(crate) fn spawn_lobby(
    mut commands: Commands,
    theme: Res<UiTheme>,
    lobby: Res<Lobby>,
    mut fingerprint: ResMut<LobbyFingerprint>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let compact = windows
        .single()
        .is_ok_and(|window| is_compact_lobby(window.width()));
    fingerprint.0 = lobby_fingerprint(&lobby, compact);
    spawn_lobby_content(&mut commands, &theme, &lobby, compact);
}

pub(crate) fn spawn_lobby_content(
    commands: &mut Commands,
    theme: &UiTheme,
    lobby: &Lobby,
    compact: bool,
) {
    commands
        .spawn((ScreenRoot, screen_node(), BackgroundColor(PAPER)))
        .with_children(|root| {
            spawn_background(root);
            let mut lobby_panel = panel_node(percent(90));
            lobby_panel.max_height = percent(94);
            lobby_panel.overflow = Overflow::scroll_y();
            root.spawn((
                lobby_panel,
                BackgroundColor(Color::NONE),
                BorderColor::all(INK),
                LobbyScrollContainer,
                ScrollPosition::default(),
            ))
            .with_children(|panel| {
                spawn_title(panel, theme, "RACERS", 42.0);
                panel
                    .spawn((Node {
                        width: percent(100),
                        display: Display::Grid,
                        grid_template_columns: if compact {
                            RepeatedGridTrack::minmax(
                                1,
                                MinTrackSizingFunction::Px(0.0),
                                MaxTrackSizingFunction::Fraction(1.0),
                            )
                        } else {
                            RepeatedGridTrack::minmax(
                                2,
                                MinTrackSizingFunction::Px(0.0),
                                MaxTrackSizingFunction::Fraction(1.0),
                            )
                        },
                        column_gap: px(12),
                        row_gap: px(12),
                        ..default()
                    },))
                    .with_children(|grid| {
                        if lobby.players.is_empty() {
                            grid.spawn((
                                Text::new("PRESS ANY INPUT TO JOIN"),
                                TextFont {
                                    font: theme.body_font.clone(),
                                    font_size: FontSize::Px(22.0),
                                    ..default()
                                },
                                TextColor(INK),
                            ));
                        }
                        for (slot, player) in lobby.players.iter().enumerate() {
                            let [
                                previous_profile,
                                next_profile,
                                previous_color,
                                next_color,
                                previous_pattern,
                                next_pattern,
                            ] = card_customization_actions(player.device);
                            let color = palette_color(player.color_id);
                            grid.spawn((
                                Node {
                                    min_height: px(0),
                                    padding: UiRect::all(px(14)),
                                    border: UiRect::all(px(2)),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(10),
                                    ..default()
                                },
                                BackgroundColor(color.with_alpha(if player.ready {
                                    0.10
                                } else {
                                    0.045
                                })),
                                BorderColor::all(if player.connected {
                                    color.with_alpha(if player.ready { 0.95 } else { 0.38 })
                                } else {
                                    CORAL.with_alpha(0.72)
                                }),
                            ))
                            .with_children(|card| {
                                card.spawn((Node {
                                    width: percent(100),
                                    flex_direction: FlexDirection::Column,
                                    align_items: AlignItems::Center,
                                    row_gap: px(5),
                                    ..default()
                                },))
                                    .with_children(|controls| {
                                        spawn_icon_stepper(
                                            controls,
                                            theme,
                                            (
                                                Text::new(player.display_name.clone()),
                                                TextFont {
                                                    font: theme.body_font.clone(),
                                                    font_size: FontSize::Px(20.0),
                                                    ..default()
                                                },
                                                TextColor(INK),
                                                TextLayout::justify(Justify::Center),
                                                Node {
                                                    width: px(190),
                                                    ..default()
                                                },
                                            ),
                                            previous_profile,
                                            next_profile,
                                            40 + slot as u16 * 10,
                                        );
                                        spawn_icon_stepper(
                                            controls,
                                            theme,
                                            (
                                                Node {
                                                    width: px(32),
                                                    height: px(32),
                                                    border: UiRect::all(px(2)),
                                                    ..default()
                                                },
                                                BackgroundColor(color),
                                                BorderColor::all(INK),
                                            ),
                                            previous_color,
                                            next_color,
                                            42 + slot as u16 * 10,
                                        );
                                        spawn_icon_stepper(
                                            controls,
                                            theme,
                                            (
                                                Text::new(format!(
                                                    "PATTERN {}",
                                                    player.pattern_id + 1
                                                )),
                                                TextFont {
                                                    font: theme.body_font.clone(),
                                                    font_size: FontSize::Px(12.0),
                                                    ..default()
                                                },
                                                TextColor(MUTED),
                                                TextLayout::justify(Justify::Center),
                                                Node {
                                                    width: px(88),
                                                    ..default()
                                                },
                                            ),
                                            previous_pattern,
                                            next_pattern,
                                            44 + slot as u16 * 10,
                                        );
                                        spawn_mini_button(
                                            controls,
                                            theme,
                                            "+ NEW",
                                            UiAction::CreateProfile(slot),
                                            46 + slot as u16 * 10,
                                        );
                                    });
                                if player.connected {
                                    let mut ready_row = card.spawn((
                                        Node {
                                            width: percent(100),
                                            min_height: px(52),
                                            display: Display::Flex,
                                            align_items: AlignItems::Center,
                                            justify_content: JustifyContent::Center,
                                            column_gap: px(12),
                                            border_radius: BorderRadius::all(px(4)),
                                            ..default()
                                        },
                                        BackgroundColor(if player.ready {
                                            Color::NONE
                                        } else {
                                            color.with_alpha(0.08)
                                        }),
                                    ));
                                    if !player.ready {
                                        ready_row.insert(ReadyPrompt {
                                            color,
                                            phase: slot as f32 * 1.7,
                                        });
                                    }
                                    ready_row.with_children(|ready| {
                                        spawn_device_icon(ready, player.device, color);
                                        spawn_button(
                                            ready,
                                            theme,
                                            if player.ready { "UNREADY" } else { "READY" },
                                            UiAction::Lobby(LobbyCommand::ToggleReady(
                                                player.device,
                                            )),
                                            48 + slot as u16 * 10,
                                        );
                                    });
                                } else {
                                    card.spawn((
                                        Text::new("PRESS ANY BUTTON TO RECLAIM"),
                                        TextFont {
                                            font: theme.body_font.clone(),
                                            font_size: FontSize::Px(13.0),
                                            ..default()
                                        },
                                        TextColor(CORAL),
                                        TextLayout::justify(Justify::Center),
                                        Node {
                                            width: percent(100),
                                            ..default()
                                        },
                                    ));
                                }
                            });
                        }
                    });
                if lobby.players.len() < MAX_HUMANS
                    && lobby.slot_for_device(InputDeviceId::Mouse).is_none()
                {
                    spawn_button(
                        panel,
                        theme,
                        "JOIN WITH MOUSE",
                        UiAction::Lobby(LobbyCommand::Join(InputDeviceId::Mouse)),
                        19,
                    );
                }
                panel
                    .spawn((Node {
                        width: percent(94),
                        max_width: px(620),
                        display: Display::Flex,
                        column_gap: px(10),
                        align_items: AlignItems::Center,
                        align_self: AlignSelf::Center,
                        ..default()
                    },))
                    .with_children(|row| {
                        spawn_mini_button(
                            row,
                            theme,
                            "-",
                            UiAction::Lobby(LobbyCommand::ChangeNpcCount(-1)),
                            20,
                        );
                        row.spawn((
                            Text::new(format!("{}", lobby.npc_count())),
                            TextFont {
                                font: theme.body_font.clone(),
                                font_size: FontSize::Px(22.0),
                                ..default()
                            },
                            TextColor(INK),
                            Node {
                                width: px(34),
                                ..default()
                            },
                            TextLayout::justify(Justify::Center),
                        ));
                        spawn_robot_icon(row, MUTED);
                        spawn_mini_button(
                            row,
                            theme,
                            "+",
                            UiAction::Lobby(LobbyCommand::ChangeNpcCount(1)),
                            21,
                        );
                        row.spawn((
                            Text::new("CPU DIFFICULTY"),
                            TextFont {
                                font: theme.body_font.clone(),
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(MUTED),
                            Node {
                                margin: UiRect::left(px(8)),
                                ..default()
                            },
                        ));
                        spawn_mini_button(
                            row,
                            theme,
                            "<",
                            UiAction::Lobby(LobbyCommand::ChangeNpcDifficulty(-1)),
                            22,
                        );
                        row.spawn((
                            Text::new(lobby.npc_difficulty.label()),
                            TextFont {
                                font: theme.body_font.clone(),
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(INK),
                            Node {
                                width: px(64),
                                ..default()
                            },
                            TextLayout::justify(Justify::Center),
                        ));
                        spawn_mini_button(
                            row,
                            theme,
                            ">",
                            UiAction::Lobby(LobbyCommand::ChangeNpcDifficulty(1)),
                            23,
                        );
                    });
                let start_label = if lobby.can_start() {
                    "START RACE"
                } else {
                    "READY UP"
                };
                spawn_button(
                    panel,
                    theme,
                    start_label,
                    UiAction::Lobby(LobbyCommand::Start),
                    24,
                );
                spawn_button(panel, theme, "BACK", UiAction::Back(AppState::Home), 25);
            });
        });
}

pub(crate) fn lobby_fingerprint(lobby: &Lobby, compact: bool) -> String {
    let players = lobby
        .players
        .iter()
        .map(|player| {
            format!(
                "{:?}:{}:{}:{}:{}:{}:{}",
                player.device,
                player.display_name,
                player.color_id,
                player.pattern_id,
                player.ready,
                player.connected,
                player.profile_id.as_deref().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    format!(
        "{}:{:?}:{}:{players}",
        lobby.npc_count(),
        lobby.npc_difficulty,
        compact
    )
}

const fn is_compact_lobby(width: f32) -> bool {
    // Two cards still fit comfortably at the 960px browser stress width. A
    // premature one-column switch made the primary start/back controls fall
    // below the viewport, so reserve stacking for genuinely narrow canvases.
    width < 840.0
}

#[derive(Component)]
pub(crate) struct LobbyScrollContainer;

pub(crate) fn scroll_lobby(
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut scroll: Query<(&mut ScrollPosition, &Node, &ComputedNode), With<LobbyScrollContainer>>,
) {
    let Ok((mut position, node, computed)) = scroll.single_mut() else {
        return;
    };
    let mut delta = 0.0;
    for event in mouse_wheel.read() {
        let amount = match event.unit {
            MouseScrollUnit::Line => event.y * 24.0,
            MouseScrollUnit::Pixel => event.y,
        };
        delta -= amount;
    }
    if delta == 0.0 || node.overflow.y != OverflowAxis::Scroll {
        return;
    }
    position.y = clamp_lobby_scroll(
        position.y,
        delta,
        computed.content_size().y,
        computed.size().y,
        computed.inverse_scale_factor,
    );
}

fn clamp_lobby_scroll(
    current: f32,
    delta: f32,
    content_height: f32,
    viewport_height: f32,
    inverse_scale_factor: f32,
) -> f32 {
    let max_offset = (content_height - viewport_height).max(0.0) * inverse_scale_factor;
    (current + delta).clamp(0.0, max_offset)
}

pub(crate) fn update_lobby_screen(
    mut commands: Commands,
    theme: Res<UiTheme>,
    lobby: Res<Lobby>,
    mut fingerprint: ResMut<LobbyFingerprint>,
    roots: Query<Entity, With<ScreenRoot>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let compact = windows
        .single()
        .is_ok_and(|window| is_compact_lobby(window.width()));
    let next = lobby_fingerprint(&lobby, compact);
    if next == fingerprint.0 {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    fingerprint.0 = next;
    spawn_lobby_content(&mut commands, &theme, &lobby, compact);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_focus_exposes_every_card_customization_command() {
        let device = InputDeviceId::Gamepad(7);
        let actions = card_customization_actions(device);
        assert!(matches!(actions[0], LobbyCommand::CycleProfile(d, -1) if d == device));
        assert!(matches!(actions[1], LobbyCommand::CycleProfile(d, 1) if d == device));
        assert!(matches!(actions[2], LobbyCommand::CycleColor(d, -1) if d == device));
        assert!(matches!(actions[3], LobbyCommand::CycleColor(d, 1) if d == device));
        assert!(matches!(actions[4], LobbyCommand::CyclePattern(d, -1) if d == device));
        assert!(matches!(actions[5], LobbyCommand::CyclePattern(d, 1) if d == device));
    }

    #[test]
    fn lobby_keeps_controls_visible_at_browser_stress_widths() {
        assert!(is_compact_lobby(800.0));
        assert!(!is_compact_lobby(960.0));
        assert!(!is_compact_lobby(1280.0));
    }

    #[test]
    fn lobby_scroll_is_clamped_to_content_bounds() {
        assert_eq!(clamp_lobby_scroll(0.0, -100.0, 900.0, 600.0, 1.0), 0.0);
        assert_eq!(clamp_lobby_scroll(100.0, 500.0, 900.0, 600.0, 1.0), 300.0);
        assert_eq!(clamp_lobby_scroll(0.0, 100.0, 500.0, 600.0, 1.0), 0.0);
    }

    #[test]
    fn fresh_two_player_lobby_starts_without_robots() {
        let profiles = ProfileStore::default();
        let mut lobby = Lobby::default();
        lobby.join(InputDeviceId::Mouse, &profiles);
        lobby.join(InputDeviceId::KeyboardPrimary, &profiles);
        assert_eq!(lobby.npc_count(), 0);
        assert_eq!(lobby.max_npc_count(), 10);
    }
}
