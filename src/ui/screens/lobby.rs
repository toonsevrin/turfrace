//! Device registration cards and match-composition controls.

use super::super::*;

fn card_customization_actions(device: InputDeviceId) -> [LobbyCommand; 4] {
    [
        LobbyCommand::CycleProfile(device, -1),
        LobbyCommand::CycleProfile(device, 1),
        LobbyCommand::CyclePattern(device, -1),
        LobbyCommand::CyclePattern(device, 1),
    ]
}
pub(crate) fn spawn_lobby(
    mut commands: Commands,
    theme: Res<UiTheme>,
    lobby: Res<Lobby>,
    mut fingerprint: ResMut<LobbyFingerprint>,
) {
    fingerprint.0 = lobby_fingerprint(&lobby);
    spawn_lobby_content(&mut commands, &theme, &lobby);
}

pub(crate) fn spawn_lobby_content(commands: &mut Commands, theme: &UiTheme, lobby: &Lobby) {
    commands
        .spawn((ScreenRoot, screen_node(), BackgroundColor(PAPER)))
        .with_children(|root| {
            spawn_background(root);
            let mut lobby_panel = panel_node(percent(90));
            lobby_panel.max_height = percent(94);
            lobby_panel.overflow = Overflow::scroll_y();
            root.spawn((lobby_panel, BackgroundColor(PANEL), BorderColor::all(INK)))
                .with_children(|panel| {
                    spawn_title(panel, theme, "LOBBY", 42.0);
                    spawn_subtitle(panel, theme, "A / ENTER TO JOIN   ·   L/R: COLOR");
                    panel
                        .spawn((Node {
                            width: percent(100),
                            display: Display::Grid,
                            grid_template_columns: RepeatedGridTrack::minmax(
                                2,
                                MinTrackSizingFunction::Auto,
                                MaxTrackSizingFunction::Fraction(1.0),
                            ),
                            column_gap: px(12),
                            row_gap: px(12),
                            ..default()
                        },))
                        .with_children(|grid| {
                            if lobby.players.is_empty() {
                                grid.spawn((
                                    Text::new("PRESS A TO JOIN"),
                                    TextFont {
                                        font: theme.font.clone(),
                                        font_size: FontSize::Px(22.0),
                                        ..default()
                                    },
                                    TextColor(Color::WHITE),
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
                                        padding: UiRect::all(px(10)),
                                        border: UiRect::all(px(2)),
                                        border_radius: BorderRadius::all(px(0)),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(4),
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
                                        "PRESS A"
                                    };
                                    card.spawn((
                                        Text::new(format!(
                                            "P{}  {}",
                                            slot + 1,
                                            player.display_name
                                        )),
                                        TextFont {
                                            font: theme.font.clone(),
                                            font_size: FontSize::Px(20.0),
                                            ..default()
                                        },
                                        TextColor(Color::WHITE),
                                    ));
                                    card.spawn((
                                        Text::new(format!(
                                            "PAT {}  ·  {status}",
                                            player.pattern_id + 1
                                        )),
                                        TextFont {
                                            font: theme.font.clone(),
                                            font_size: FontSize::Px(13.0),
                                            ..default()
                                        },
                                        TextColor(if player.ready { LIME } else { MUTED }),
                                    ));
                                    card.spawn((Node {
                                        display: Display::Flex,
                                        column_gap: px(6),
                                        ..default()
                                    },))
                                        .with_children(|row| {
                                            spawn_mini_button(
                                                row,
                                                theme,
                                                "PROF ◀",
                                                UiAction::Lobby(previous_profile),
                                                40 + slot as u16 * 10,
                                            );
                                            spawn_mini_button(
                                                row,
                                                theme,
                                                "PROF ▶",
                                                UiAction::Lobby(next_profile),
                                                41 + slot as u16 * 10,
                                            );
                                        });
                                    card.spawn((Node {
                                        display: Display::Flex,
                                        column_gap: px(6),
                                        ..default()
                                    },))
                                        .with_children(|row| {
                                            spawn_mini_button(
                                                row,
                                                theme,
                                                "PAT ◀",
                                                UiAction::Lobby(previous_pattern),
                                                42 + slot as u16 * 10,
                                            );
                                            spawn_mini_button(
                                                row,
                                                theme,
                                                "PAT ▶",
                                                UiAction::Lobby(next_pattern),
                                                43 + slot as u16 * 10,
                                            );
                                        });
                                    spawn_mini_button(
                                        card,
                                        theme,
                                        "NEW PROFILE",
                                        UiAction::CreateProfile(slot),
                                        44 + slot as u16 * 10,
                                    );
                                });
                            }
                        });
                    panel
                        .spawn((Node {
                            width: percent(100),
                            display: Display::Flex,
                            column_gap: px(10),
                            align_items: AlignItems::Center,
                            ..default()
                        },))
                        .with_children(|row| {
                            spawn_mini_button(
                                row,
                                theme,
                                "−",
                                UiAction::Lobby(LobbyCommand::ChangeCompetitors(-1)),
                                20,
                            );
                            row.spawn((
                                Text::new(format!(
                                    "PLAYERS {}  ·  CPU {}",
                                    lobby.total_competitors,
                                    lobby.npc_count()
                                )),
                                TextFont {
                                    font: theme.font.clone(),
                                    font_size: FontSize::Px(17.0),
                                    ..default()
                                },
                                TextColor(Color::WHITE),
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
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
                        "READY 2 TO START"
                    };
                    spawn_button(
                        panel,
                        theme,
                        start_label,
                        UiAction::Lobby(LobbyCommand::Start),
                        22,
                    );
                    spawn_button(panel, theme, "BACK", UiAction::State(AppState::Home), 23);
                    panel.spawn((
                        Text::new(""),
                        TextFont {
                            font: theme.font.clone(),
                            font_size: FontSize::Px(42.0),
                            ..default()
                        },
                        TextColor(LIME),
                        TextLayout::justify(Justify::Center),
                        LobbyCountdownText,
                    ));
                });
        });
}

pub(crate) fn lobby_fingerprint(lobby: &Lobby) -> String {
    let players = lobby
        .players
        .iter()
        .map(|player| {
            format!(
                "{}:{}:{}:{}:{}:{}",
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
    format!("{}:{players}", lobby.total_competitors)
}

pub(crate) fn update_lobby_screen(
    mut commands: Commands,
    theme: Res<UiTheme>,
    lobby: Res<Lobby>,
    mut fingerprint: ResMut<LobbyFingerprint>,
    roots: Query<Entity, With<ScreenRoot>>,
) {
    let next = lobby_fingerprint(&lobby);
    if next == fingerprint.0 {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    fingerprint.0 = next;
    spawn_lobby_content(&mut commands, &theme, &lobby);
}

pub(crate) fn update_lobby_countdown(
    lobby: Res<Lobby>,
    mut text: Query<&mut Text, With<LobbyCountdownText>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    text.0 = lobby
        .countdown_remaining
        .map_or_else(String::new, |remaining| {
            format!("STARTING IN {}", remaining.ceil() as u8)
        });
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
}
