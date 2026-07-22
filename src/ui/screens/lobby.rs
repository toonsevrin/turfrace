//! Device registration cards and match-composition controls.

use super::super::*;
use crate::lobby::MAX_HUMANS;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::window::PrimaryWindow;

fn input_device_label(device: InputDeviceId) -> String {
    match device {
        InputDeviceId::KeyboardPrimary => "KEYBOARD".to_owned(),
        InputDeviceId::Mouse => "MOUSE".to_owned(),
        InputDeviceId::Gamepad(id) => format!("CONTROLLER {}", id + 1),
    }
}

fn card_customization_actions(device: InputDeviceId) -> [LobbyCommand; 4] {
    [
        LobbyCommand::CycleProfile(device, -1),
        LobbyCommand::CycleProfile(device, 1),
        LobbyCommand::CyclePattern(device, -1),
        LobbyCommand::CyclePattern(device, 1),
    ]
}

fn spawn_card_stepper(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: &str,
    previous: LobbyCommand,
    next: LobbyCommand,
    order: u16,
) {
    parent
        .spawn((Node {
            display: Display::Flex,
            column_gap: px(8),
            align_items: AlignItems::Center,
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font: theme.body_font.clone(),
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(MUTED),
                Node {
                    width: px(64),
                    ..default()
                },
            ));
            spawn_mini_button(row, theme, "<", UiAction::Lobby(previous), order);
            spawn_mini_button(row, theme, ">", UiAction::Lobby(next), order + 1);
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
                spawn_title(panel, theme, "LOBBY", 42.0);
                spawn_subtitle(panel, theme, "KEYBOARD: ENTER / ARROWS / WASD");
                spawn_subtitle(
                    panel,
                    theme,
                    "MOUSE: JOIN WITH MOUSE   /   CONTROLLER: PRESS A",
                );
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
                                previous_pattern,
                                next_pattern,
                            ] = card_customization_actions(player.device);
                            let color = palette_color(player.color_id);
                            grid.spawn((
                                Node {
                                    min_height: px(0),
                                    padding: UiRect::all(px(12)),
                                    border: UiRect::all(px(2)),
                                    border_radius: BorderRadius::all(px(0)),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(6),
                                    ..default()
                                },
                                BackgroundColor(color.with_alpha(0.20)),
                                BorderColor::all(if player.connected { color } else { CORAL }),
                            ))
                            .with_children(|card| {
                                let status = if !player.connected {
                                    "OFFLINE"
                                } else if player.ready {
                                    "READY"
                                } else {
                                    "NOT READY"
                                };
                                card.spawn((Node {
                                    width: percent(100),
                                    display: Display::Flex,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },))
                                    .with_children(|header| {
                                        header.spawn((
                                            Text::new(format!(
                                                "P{}  {}  /  {}",
                                                slot + 1,
                                                player.display_name,
                                                input_device_label(player.device),
                                            )),
                                            TextFont {
                                                font: theme.body_font.clone(),
                                                font_size: FontSize::Px(20.0),
                                                ..default()
                                            },
                                            TextColor(INK),
                                            Node {
                                                flex_grow: 1.0,
                                                ..default()
                                            },
                                        ));
                                        header.spawn((
                                            Text::new(status),
                                            TextFont {
                                                font: theme.body_font.clone(),
                                                font_size: FontSize::Px(12.0),
                                                ..default()
                                            },
                                            TextColor(if player.ready { LIME } else { MUTED }),
                                        ));
                                    });
                                card.spawn((Node {
                                    width: percent(100),
                                    display: Display::Flex,
                                    align_items: AlignItems::Center,
                                    column_gap: px(16),
                                    ..default()
                                },))
                                    .with_children(|body| {
                                        body.spawn((Node {
                                            flex_grow: 1.0,
                                            flex_direction: FlexDirection::Column,
                                            row_gap: px(6),
                                            ..default()
                                        },))
                                            .with_children(|controls| {
                                                spawn_card_stepper(
                                                    controls,
                                                    theme,
                                                    "PROFILE",
                                                    previous_profile,
                                                    next_profile,
                                                    40 + slot as u16 * 10,
                                                );
                                                spawn_card_stepper(
                                                    controls,
                                                    theme,
                                                    &format!("PATTERN {}", player.pattern_id + 1),
                                                    previous_pattern,
                                                    next_pattern,
                                                    42 + slot as u16 * 10,
                                                );
                                                spawn_mini_button(
                                                    controls,
                                                    theme,
                                                    "NEW PROFILE",
                                                    UiAction::CreateProfile(slot),
                                                    44 + slot as u16 * 10,
                                                );
                                                if player.connected {
                                                    spawn_mini_button(
                                                        controls,
                                                        theme,
                                                        if player.ready {
                                                            "UNREADY"
                                                        } else {
                                                            "READY UP"
                                                        },
                                                        UiAction::Lobby(LobbyCommand::ToggleReady(
                                                            player.device,
                                                        )),
                                                        46 + slot as u16 * 10,
                                                    );
                                                }
                                            });
                                        body.spawn((
                                            Node {
                                                width: px(82),
                                                height: px(82),
                                                border: UiRect::all(px(2)),
                                                align_items: AlignItems::Center,
                                                justify_content: JustifyContent::Center,
                                                ..default()
                                            },
                                            BackgroundColor(color),
                                            BorderColor::all(INK),
                                        ))
                                        .with_children(
                                            |stamp| {
                                                stamp.spawn((
                                                    Text::new(format!("P{}", slot + 1)),
                                                    TextFont {
                                                        font: theme.display_font.clone(),
                                                        font_size: FontSize::Px(26.0),
                                                        ..default()
                                                    },
                                                    TextColor(CREAM),
                                                    TextShadow {
                                                        offset: Vec2::new(3.0, 3.0),
                                                        color: INK,
                                                    },
                                                ));
                                            },
                                        );
                                    });
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
                        width: percent(76),
                        max_width: px(360),
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
                            UiAction::Lobby(LobbyCommand::ChangeCompetitors(-1)),
                            20,
                        );
                        row.spawn((
                            Text::new(format!(
                                "{} PLAYERS  /  {} CPU",
                                lobby.total_competitors,
                                lobby.npc_count()
                            )),
                            TextFont {
                                font: theme.body_font.clone(),
                                font_size: FontSize::Px(17.0),
                                ..default()
                            },
                            TextColor(INK),
                            Node {
                                flex_grow: 1.0,
                                ..default()
                            },
                            TextLayout::justify(Justify::Center),
                        ));
                        spawn_mini_button(
                            row,
                            theme,
                            "+",
                            UiAction::Lobby(LobbyCommand::ChangeCompetitors(1)),
                            21,
                        );
                    });
                let start_label = if lobby.can_start() {
                    "START MATCH"
                } else {
                    "READY 1 TO START"
                };
                spawn_button(
                    panel,
                    theme,
                    start_label,
                    UiAction::Lobby(LobbyCommand::Start),
                    22,
                );
                spawn_button(panel, theme, "BACK", UiAction::State(AppState::Home), 23);
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
    format!("{}:{}:{players}", lobby.total_competitors, compact)
}

const fn is_compact_lobby(width: f32) -> bool {
    width < 1100.0
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
        assert!(matches!(actions[2], LobbyCommand::CyclePattern(d, -1) if d == device));
        assert!(matches!(actions[3], LobbyCommand::CyclePattern(d, 1) if d == device));
    }

    #[test]
    fn lobby_uses_one_column_at_browser_stress_widths() {
        assert!(is_compact_lobby(960.0));
        assert!(!is_compact_lobby(1280.0));
    }

    #[test]
    fn lobby_scroll_is_clamped_to_content_bounds() {
        assert_eq!(clamp_lobby_scroll(0.0, -100.0, 900.0, 600.0, 1.0), 0.0);
        assert_eq!(clamp_lobby_scroll(100.0, 500.0, 900.0, 600.0, 1.0), 300.0);
        assert_eq!(clamp_lobby_scroll(0.0, 100.0, 500.0, 600.0, 1.0), 0.0);
    }

    #[test]
    fn lobby_labels_each_supported_input_device() {
        assert_eq!(
            input_device_label(InputDeviceId::KeyboardPrimary),
            "KEYBOARD"
        );
        assert_eq!(input_device_label(InputDeviceId::Mouse), "MOUSE");
        assert_eq!(
            input_device_label(InputDeviceId::Gamepad(1)),
            "CONTROLLER 2"
        );
    }
}
