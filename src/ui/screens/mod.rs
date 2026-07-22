//! Application screens other than the device-registration lobby.

mod lobby;
pub(super) use lobby::*;

use super::*;

pub(super) fn spawn_home(mut commands: Commands, theme: Res<UiTheme>) {
    commands
        .spawn((ScreenRoot, screen_node(), BackgroundColor(PAPER)))
        .with_children(|root| {
            spawn_background(root);
            let mut home_panel = panel_node(percent(90));
            home_panel.max_width = px(500);
            root.spawn((home_panel, BackgroundColor(PANEL), BorderColor::all(INK)))
                .with_children(|panel| {
                    spawn_title(panel, &theme, "TURFRACE", 68.0);
                    spawn_subtitle(panel, &theme, "CUT  •  CLAIM  •  SURVIVE");
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

// Lobby screen systems live in `screens::lobby`.
pub(super) fn spawn_leaderboard(
    mut commands: Commands,
    theme: Res<UiTheme>,
    profiles: Res<ProfileStore>,
) {
    commands
        .spawn((ScreenRoot, screen_node(), BackgroundColor(PAPER)))
        .with_children(|root| {
            spawn_background(root);
            root.spawn((
                panel_node(percent(82)),
                BackgroundColor(PANEL),
                BorderColor::all(INK),
            ))
            .with_children(|panel| {
                spawn_title(panel, &theme, "LEADERBOARD", 42.0);
                spawn_subtitle(panel, &theme, "LOCAL RECORDS");
                let leaderboard = profiles.sorted_leaderboard();
                if leaderboard.is_empty() {
                    spawn_subtitle(panel, &theme, "NO MATCHES YET");
                }
                for (rank, profile) in leaderboard.into_iter().enumerate() {
                    let stats = &profile.statistics;
                    let win_rate = if stats.games_played == 0 {
                        0.0
                    } else {
                        stats.wins as f32 * 100.0 / stats.games_played as f32
                    };
                    panel
                        .spawn((
                            Node {
                                width: percent(100),
                                min_height: px(38),
                                padding: UiRect::axes(px(10), px(6)),
                                border: UiRect::bottom(px(1)),
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(if rank == 0 {
                                Color::srgba(0.68, 0.92, 0.22, 0.09)
                            } else {
                                Color::srgba(0.01, 0.015, 0.025, 0.16)
                            }),
                            BorderColor::all(Color::srgba(0.40, 0.52, 0.65, 0.34)),
                        ))
                        .with_children(|row| {
                            row.spawn((
                                Text::new(format!("#{:02}", rank + 1)),
                                TextFont {
                                    font: theme.font.clone(),
                                    font_size: FontSize::Px(18.0),
                                    ..default()
                                },
                                TextColor(if rank == 0 { LIME } else { MUTED }),
                                Node {
                                    width: px(52),
                                    ..default()
                                },
                            ));
                            row.spawn((
                                Text::new(format!(
                                    "{}   W{} / G{}   {:.0}%   K{}",
                                    profile.display_name,
                                    stats.wins,
                                    stats.games_played,
                                    win_rate,
                                    stats.kills
                                )),
                                TextFont {
                                    font: theme.font.clone(),
                                    font_size: FontSize::Px(17.0),
                                    ..default()
                                },
                                TextColor(if rank == 0 { Color::WHITE } else { MUTED }),
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                        });
                }
                spawn_button(panel, &theme, "BACK", UiAction::State(AppState::Home), 0);
            });
        });
}

pub(super) fn setting_line(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: &str,
    value: String,
    field: SettingField,
    order: u16,
) {
    parent
        .spawn((Node {
            width: percent(100),
            display: Display::Flex,
            column_gap: px(10),
            align_items: AlignItems::Center,
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
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
            spawn_button(row, theme, "−", UiAction::AdjustSetting(field, -0.1), order);
            row.spawn((
                Text::new(value),
                TextFont {
                    font: theme.font.clone(),
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(LIME),
                Node {
                    width: px(72),
                    ..default()
                },
                SettingsValueText(field),
            ));
            spawn_button(
                row,
                theme,
                "+",
                UiAction::AdjustSetting(field, 0.1),
                order + 1,
            );
        });
}

pub(super) fn toggle_line(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: &str,
    enabled: bool,
    field: SettingField,
    order: u16,
) {
    spawn_button(
        parent,
        theme,
        format!("{label}: {}", if enabled { "ON" } else { "OFF" }),
        UiAction::ToggleSetting(field),
        order,
    );
}

pub(super) fn spawn_settings(
    mut commands: Commands,
    theme: Res<UiTheme>,
    settings: Res<UserSettings>,
    profiles: Res<ProfileStore>,
    persistence: Res<PersistenceStatus>,
    confirmation: Res<Confirmation>,
) {
    commands
        .spawn((ScreenRoot, screen_node(), BackgroundColor(PAPER)))
        .with_children(|root| {
            spawn_background(root);
            let mut settings_panel = panel_node(percent(78));
            settings_panel.max_height = percent(96);
            settings_panel.overflow = Overflow::scroll_y();
            root.spawn((
                settings_panel,
                BackgroundColor(PANEL),
                BorderColor::all(INK),
            ))
            .with_children(|panel| {
                spawn_title(panel, &theme, "SETTINGS", 43.0);
                setting_line(
                    panel,
                    &theme,
                    "MASTER",
                    format!("{:>3}%", (settings.master_volume * 100.0).round()),
                    SettingField::Master,
                    0,
                );
                setting_line(
                    panel,
                    &theme,
                    "MUSIC",
                    format!("{:>3}%", (settings.music_volume * 100.0).round()),
                    SettingField::Music,
                    2,
                );
                setting_line(
                    panel,
                    &theme,
                    "SFX",
                    format!("{:>3}%", (settings.sound_effect_volume * 100.0).round()),
                    SettingField::SoundEffects,
                    4,
                );
                setting_line(
                    panel,
                    &theme,
                    "SHAKE",
                    format!("{:>3}%", (settings.screen_shake * 100.0).round()),
                    SettingField::ScreenShake,
                    6,
                );
                toggle_line(
                    panel,
                    &theme,
                    "MOTION",
                    settings.reduced_motion,
                    SettingField::ReducedMotion,
                    8,
                );
                toggle_line(
                    panel,
                    &theme,
                    "PATTERNS",
                    settings.colorblind_assist,
                    SettingField::Colorblind,
                    9,
                );
                toggle_line(
                    panel,
                    &theme,
                    "FULLSCREEN",
                    settings.fullscreen,
                    SettingField::Fullscreen,
                    10,
                );
                setting_line(
                    panel,
                    &theme,
                    "DEADZONE",
                    format!("{:>3}%", (settings.gamepad_deadzone * 100.0).round()),
                    SettingField::GamepadDeadzone,
                    11,
                );
                setting_line(
                    panel,
                    &theme,
                    "AIM SPEED",
                    format!("{:.1}×", settings.mouse_sensitivity),
                    SettingField::MouseSensitivity,
                    13,
                );
                toggle_line(
                    panel,
                    &theme,
                    "BIG HUD",
                    settings.larger_hud_text,
                    SettingField::LargerHudText,
                    15,
                );
                toggle_line(
                    panel,
                    &theme,
                    "HIGH CONTRAST",
                    settings.high_contrast_ui,
                    SettingField::HighContrast,
                    16,
                );
                spawn_button(
                    panel,
                    &theme,
                    format!("QUALITY: {:?}", settings.graphics_quality).to_uppercase(),
                    UiAction::ToggleSetting(SettingField::GraphicsQuality),
                    17,
                );
                if !profiles.profiles.is_empty() {
                    spawn_subtitle(
                        panel,
                        &theme,
                        format!("PROFILES  ·  {}", profiles.profiles.len()),
                    );
                    for (index, profile) in profiles.profiles.iter().enumerate() {
                        panel
                            .spawn((Node {
                                display: Display::Flex,
                                column_gap: px(8),
                                align_items: AlignItems::Center,
                                ..default()
                            },))
                            .with_children(|row| {
                                row.spawn((
                                    Text::new(profile.display_name.clone()),
                                    TextFont {
                                        font: theme.font.clone(),
                                        font_size: FontSize::Px(16.0),
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
                                    &theme,
                                    "RENAME",
                                    UiAction::RenameProfile(profile.id.clone()),
                                    30 + index as u16 * 2,
                                );
                                let delete_label = if confirmation.action
                                    == Some(ConfirmationAction::DeleteProfile(profile.id.clone()))
                                {
                                    "CONFIRM"
                                } else {
                                    "DELETE"
                                };
                                spawn_mini_button(
                                    row,
                                    &theme,
                                    delete_label,
                                    UiAction::DeleteProfile(profile.id.clone()),
                                    31 + index as u16 * 2,
                                );
                            });
                    }
                    let reset = if confirmation.action == Some(ConfirmationAction::ResetStatistics)
                    {
                        "CONFIRM RESET"
                    } else {
                        "RESET STATS"
                    };
                    let all = if confirmation.action == Some(ConfirmationAction::ResetAllData) {
                        "CONFIRM RESET"
                    } else {
                        "RESET DATA"
                    };
                    spawn_button(panel, &theme, reset, UiAction::ResetStatistics, 18);
                    spawn_button(panel, &theme, all, UiAction::ResetAllData, 19);
                }
                if let Some(warning) = &persistence.warning {
                    panel.spawn((
                        Text::new(warning.clone()),
                        TextFont {
                            font: theme.font.clone(),
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(Color::srgb(1.0, 0.65, 0.20)),
                    ));
                }
                spawn_button(panel, &theme, "BACK", UiAction::SettingsBack, 20);
            });
        });
}

pub(super) fn spawn_pause(
    mut commands: Commands,
    theme: Res<UiTheme>,
    disconnect: Res<MatchDisconnectNotice>,
) {
    commands
        .spawn((
            ScreenRoot,
            screen_node(),
            BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.82)),
        ))
        .with_children(|root| {
            let mut pause_panel = panel_node(percent(90));
            pause_panel.max_width = px(530);
            root.spawn((pause_panel, BackgroundColor(PANEL), BorderColor::all(INK)))
                .with_children(|panel| {
                    spawn_title(panel, &theme, "PAUSED", 58.0);
                    if disconnect.device.is_some() {
                        spawn_subtitle(
                            panel,
                            &theme,
                            format!("{} LOST CONTROLLER", disconnect.player_name),
                        );
                        spawn_subtitle(panel, &theme, "PRESS A TO TAKE OVER");
                        spawn_button(
                            panel,
                            &theme,
                            "REPLACE WITH CPU",
                            UiAction::ReplaceDisconnected,
                            0,
                        );
                    } else {
                        spawn_button(panel, &theme, "RESUME", UiAction::Resume, 0);
                    }
                    spawn_button(panel, &theme, "SETTINGS", UiAction::Settings, 1);
                    spawn_button(
                        panel,
                        &theme,
                        "RECONNECT",
                        UiAction::State(AppState::Paused),
                        2,
                    );
                    spawn_button(
                        panel,
                        &theme,
                        "RESTART MATCH",
                        UiAction::State(AppState::MatchLoading),
                        3,
                    );
                    spawn_button(
                        panel,
                        &theme,
                        "RETURN TO LOBBY",
                        UiAction::State(AppState::Lobby),
                        4,
                    );
                });
        });
}

pub(super) fn spawn_game_over(
    mut commands: Commands,
    theme: Res<UiTheme>,
    results: Res<MatchResults>,
) {
    commands
        .spawn((
            ScreenRoot,
            screen_node(),
            BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.5)),
        ))
        .with_children(|root| {
            let mut game_over_panel = panel_node(percent(90));
            game_over_panel.max_width = px(620);
            root.spawn((
                game_over_panel,
                BackgroundColor(PANEL),
                BorderColor::all(INK),
            ))
            .with_children(|panel| {
                spawn_title(panel, &theme, "100% CLAIMED!", 52.0);
                spawn_subtitle(
                    panel,
                    &theme,
                    format!(
                        "{} OWNS THE FIELD",
                        if results.winner_name.is_empty() {
                            "THE WINNER"
                        } else {
                            &results.winner_name
                        }
                    ),
                );
                spawn_button(
                    panel,
                    &theme,
                    "VIEW RESULTS",
                    UiAction::State(AppState::Results),
                    0,
                );
            });
        });
}

pub(super) fn spawn_results(
    mut commands: Commands,
    theme: Res<UiTheme>,
    results: Res<MatchResults>,
) {
    commands
        .spawn((ScreenRoot, screen_node(), BackgroundColor(PAPER)))
        .with_children(|root| {
            spawn_background(root);
            let mut results_panel = panel_node(percent(86));
            results_panel.max_height = percent(94);
            // Eight-player results must fit a standard 720p canvas without
            // hiding the final navigation action below the fold. Narrower
            // windows still retain scrolling as a fallback.
            results_panel.padding = UiRect::axes(px(30), px(10));
            results_panel.row_gap = px(4);
            results_panel.overflow = Overflow::scroll_y();
            root.spawn((results_panel, BackgroundColor(PANEL), BorderColor::all(INK)))
                .with_children(|panel| {
                    spawn_title(panel, &theme, "RESULTS", 40.0);
                    spawn_subtitle(
                        panel,
                        &theme,
                        format!(
                            "{}  ·  {:02}:{:02}",
                            if results.winner_name.is_empty() {
                                "NO WINNER"
                            } else {
                                &results.winner_name
                            },
                            (results.duration_seconds / 60.0) as u32,
                            results.duration_seconds as u32 % 60
                        ),
                    );
                    panel.spawn((
                        Text::new("RANK / PLAYER        PEAK   K/D   CAP   CELLS   TRAIL"),
                        TextFont {
                            font: theme.font.clone(),
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(MUTED),
                    ));
                    if results.rows.is_empty() {
                        spawn_subtitle(panel, &theme, "NO MATCH DATA");
                    }
                    for row in &results.rows {
                        panel
                            .spawn((
                                Node {
                                    width: percent(100),
                                    min_height: px(28),
                                    padding: UiRect::axes(px(8), px(2)),
                                    border: UiRect::bottom(px(1)),
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(if row.placement == 1 {
                                    Color::srgba(0.68, 0.92, 0.22, 0.09)
                                } else {
                                    Color::srgba(0.01, 0.015, 0.025, 0.12)
                                }),
                                BorderColor::all(Color::srgba(0.40, 0.52, 0.65, 0.30)),
                            ))
                            .with_children(|line| {
                                line.spawn((
                                    Text::new(format!(
                                        "#{:02}  {}   {:.0}%   {}/{}   CAP {:.0}%   {}K   {:.0}M",
                                        row.placement,
                                        row.name,
                                        row.peak_percent,
                                        row.kills,
                                        row.deaths,
                                        row.largest_capture_percent,
                                        (row.total_cells_captured as f32 / 1000.0).round(),
                                        row.longest_trail
                                    )),
                                    TextFont {
                                        font: theme.font.clone(),
                                        font_size: FontSize::Px(13.0),
                                        ..default()
                                    },
                                    TextColor(if row.placement == 1 {
                                        LIME
                                    } else {
                                        Color::WHITE
                                    }),
                                ));
                            });
                    }
                    spawn_mini_button(
                        panel,
                        &theme,
                        "REMATCH",
                        UiAction::Rematch { same_field: false },
                        0,
                    );
                    spawn_mini_button(
                        panel,
                        &theme,
                        "REPLAY FIELD",
                        UiAction::Rematch { same_field: true },
                        1,
                    );
                    spawn_mini_button(panel, &theme, "LOBBY", UiAction::State(AppState::Lobby), 2);
                    spawn_mini_button(panel, &theme, "HOME", UiAction::State(AppState::Home), 3);
                });
        });
}

pub(super) fn refresh_settings_labels(
    settings: Res<UserSettings>,
    confirmation: Res<Confirmation>,
    values: Query<(Entity, &SettingsValueText)>,
    buttons: Query<(&UiAction, &Children)>,
    mut texts: Query<&mut Text>,
) {
    if settings.is_changed() {
        for (entity, marker) in &values {
            let value = match marker.0 {
                SettingField::Master => settings.master_volume,
                SettingField::Music => settings.music_volume,
                SettingField::SoundEffects => settings.sound_effect_volume,
                SettingField::ScreenShake => settings.screen_shake,
                SettingField::GamepadDeadzone => settings.gamepad_deadzone,
                SettingField::MouseSensitivity => settings.mouse_sensitivity,
                _ => continue,
            };
            if let Ok(mut text) = texts.get_mut(entity) {
                text.0 = if marker.0 == SettingField::MouseSensitivity {
                    format!("{value:.1}×")
                } else {
                    format!("{:>3}%", (value * 100.0).round())
                };
            }
        }
    }
    if !settings.is_changed() && !confirmation.is_changed() {
        return;
    }
    for (action, children) in &buttons {
        let label = match action {
            UiAction::ToggleSetting(SettingField::ReducedMotion) => Some(format!(
                "MOTION: {}",
                if settings.reduced_motion { "ON" } else { "OFF" }
            )),
            UiAction::ToggleSetting(SettingField::Colorblind) => Some(format!(
                "PATTERNS: {}",
                if settings.colorblind_assist {
                    "ON"
                } else {
                    "OFF"
                }
            )),
            UiAction::ToggleSetting(SettingField::Fullscreen) => Some(format!(
                "FULLSCREEN: {}",
                if settings.fullscreen { "ON" } else { "OFF" }
            )),
            UiAction::ToggleSetting(SettingField::LargerHudText) => Some(format!(
                "BIG HUD: {}",
                if settings.larger_hud_text {
                    "ON"
                } else {
                    "OFF"
                }
            )),
            UiAction::ToggleSetting(SettingField::HighContrast) => Some(format!(
                "HIGH CONTRAST: {}",
                if settings.high_contrast_ui {
                    "ON"
                } else {
                    "OFF"
                }
            )),
            UiAction::ToggleSetting(SettingField::GraphicsQuality) => {
                Some(format!("QUALITY: {:?}", settings.graphics_quality).to_uppercase())
            }
            UiAction::ResetStatistics => Some(
                if confirmation.action == Some(ConfirmationAction::ResetStatistics) {
                    "CONFIRM RESET"
                } else {
                    "RESET STATS"
                }
                .to_owned(),
            ),
            UiAction::ResetAllData => Some(
                if confirmation.action == Some(ConfirmationAction::ResetAllData) {
                    "CONFIRM RESET"
                } else {
                    "RESET DATA"
                }
                .to_owned(),
            ),
            UiAction::DeleteProfile(profile_id) => Some(
                if confirmation.action
                    == Some(ConfirmationAction::DeleteProfile(profile_id.clone()))
                {
                    "CONFIRM"
                } else {
                    "DELETE"
                }
                .to_owned(),
            ),
            _ => None,
        };
        let Some(label) = label else { continue };
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child) {
                text.0 = label.clone();
            }
        }
    }
}

// Interaction systems live in `interaction`.

// Gameplay HUD and results projection live in `gameplay_hud`.
