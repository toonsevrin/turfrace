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
            root.spawn((
                home_panel,
                BackgroundColor(Color::NONE),
                BorderColor::all(INK),
            ))
            .with_children(|panel| {
                spawn_title(panel, &theme, "TURFRACE", 68.0);
                spawn_subtitle(panel, &theme, "CUT / CLAIM / SURVIVE");
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
                BackgroundColor(Color::NONE),
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
                            BackgroundColor(Color::NONE),
                            BorderColor::all(Color::srgba(0.40, 0.52, 0.65, 0.34)),
                        ))
                        .with_children(|row| {
                            row.spawn((
                                Text::new(format!("#{:02}", rank + 1)),
                                TextFont {
                                    font: theme.body_font.clone(),
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
                                    font: theme.body_font.clone(),
                                    font_size: FontSize::Px(17.0),
                                    ..default()
                                },
                                TextColor(if rank == 0 { INK } else { MUTED }),
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
                    font: theme.body_font.clone(),
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(INK),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            spawn_mini_button(row, theme, "-", UiAction::AdjustSetting(field, -0.1), order);
            row.spawn((
                Text::new(value),
                TextFont {
                    font: theme.body_font.clone(),
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
            spawn_mini_button(
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
    spawn_compact_menu_button(
        parent,
        theme,
        format!("{label}: {}", if enabled { "ON" } else { "OFF" }),
        UiAction::ToggleSetting(field),
        order,
    );
}

fn settings_section(parent: &mut ChildSpawnerCommands, theme: &UiTheme, label: &str) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font: theme.body_font.clone(),
            font_size: FontSize::Px(12.0),
            ..default()
        },
        TextColor(MUTED),
        Node {
            width: percent(100),
            margin: UiRect::top(px(2)),
            padding: UiRect::bottom(px(3)),
            border: UiRect::bottom(px(1)),
            ..default()
        },
        BorderColor::all(Color::srgba(0.24, 0.31, 0.40, 0.32)),
    ));
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
            settings_panel.padding = UiRect::axes(px(28), px(6));
            settings_panel.row_gap = px(6);
            settings_panel.overflow = Overflow::scroll_y();
            root.spawn((
                settings_panel,
                BackgroundColor(Color::NONE),
                BorderColor::all(INK),
            ))
            .with_children(|panel| {
                spawn_title(panel, &theme, "SETTINGS", 38.0);
                panel
                    .spawn((Node {
                        width: percent(100),
                        display: Display::Grid,
                        grid_template_columns: RepeatedGridTrack::minmax(
                            2,
                            MinTrackSizingFunction::Auto,
                            MaxTrackSizingFunction::Fraction(1.0),
                        ),
                        column_gap: px(30),
                        align_items: AlignItems::Start,
                        ..default()
                    },))
                    .with_children(|grid| {
                        grid.spawn((Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(4),
                            ..default()
                        },))
                            .with_children(|left| {
                                settings_section(left, &theme, "AUDIO");
                                setting_line(
                                    left,
                                    &theme,
                                    "MASTER",
                                    format!("{:>3}%", (settings.master_volume * 100.0).round()),
                                    SettingField::Master,
                                    0,
                                );
                                setting_line(
                                    left,
                                    &theme,
                                    "MUSIC",
                                    format!("{:>3}%", (settings.music_volume * 100.0).round()),
                                    SettingField::Music,
                                    2,
                                );
                                setting_line(
                                    left,
                                    &theme,
                                    "SFX",
                                    format!(
                                        "{:>3}%",
                                        (settings.sound_effect_volume * 100.0).round()
                                    ),
                                    SettingField::SoundEffects,
                                    4,
                                );
                                settings_section(left, &theme, "DISPLAY");
                                setting_line(
                                    left,
                                    &theme,
                                    "SHAKE",
                                    format!("{:>3}%", (settings.screen_shake * 100.0).round()),
                                    SettingField::ScreenShake,
                                    6,
                                );
                                toggle_line(
                                    left,
                                    &theme,
                                    "MOTION",
                                    settings.reduced_motion,
                                    SettingField::ReducedMotion,
                                    8,
                                );
                                toggle_line(
                                    left,
                                    &theme,
                                    "PATTERNS",
                                    settings.colorblind_assist,
                                    SettingField::Colorblind,
                                    9,
                                );
                                toggle_line(
                                    left,
                                    &theme,
                                    "FULLSCREEN",
                                    settings.fullscreen,
                                    SettingField::Fullscreen,
                                    10,
                                );
                                spawn_compact_menu_button(
                                    left,
                                    &theme,
                                    format!("QUALITY / {:?}", settings.graphics_quality)
                                        .to_uppercase(),
                                    UiAction::ToggleSetting(SettingField::GraphicsQuality),
                                    17,
                                );
                            });
                        grid.spawn((Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(4),
                            ..default()
                        },))
                            .with_children(|right| {
                                settings_section(right, &theme, "CONTROL");
                                setting_line(
                                    right,
                                    &theme,
                                    "DEADZONE",
                                    format!("{:>3}%", (settings.gamepad_deadzone * 100.0).round()),
                                    SettingField::GamepadDeadzone,
                                    11,
                                );
                                setting_line(
                                    right,
                                    &theme,
                                    "AIM SPEED",
                                    format!("{:.1}x", settings.mouse_sensitivity),
                                    SettingField::MouseSensitivity,
                                    13,
                                );
                                settings_section(right, &theme, "ACCESSIBILITY");
                                toggle_line(
                                    right,
                                    &theme,
                                    "BIG HUD",
                                    settings.larger_hud_text,
                                    SettingField::LargerHudText,
                                    15,
                                );
                                toggle_line(
                                    right,
                                    &theme,
                                    "HIGH CONTRAST",
                                    settings.high_contrast_ui,
                                    SettingField::HighContrast,
                                    16,
                                );
                                if !profiles.profiles.is_empty() {
                                    settings_section(
                                        right,
                                        &theme,
                                        &format!("PROFILES / {}", profiles.profiles.len()),
                                    );
                                    for (index, profile) in profiles.profiles.iter().enumerate() {
                                        right
                                            .spawn((Node {
                                                width: percent(100),
                                                display: Display::Flex,
                                                column_gap: px(6),
                                                align_items: AlignItems::Center,
                                                ..default()
                                            },))
                                            .with_children(|row| {
                                                row.spawn((
                                                    Text::new(profile.display_name.clone()),
                                                    TextFont {
                                                        font: theme.body_font.clone(),
                                                        font_size: FontSize::Px(14.0),
                                                        ..default()
                                                    },
                                                    TextColor(INK),
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
                                                    18 + index as u16 * 2,
                                                );
                                                let delete_label = if confirmation.action
                                                    == Some(ConfirmationAction::DeleteProfile(
                                                        profile.id.clone(),
                                                    )) {
                                                    "CONFIRM"
                                                } else {
                                                    "DELETE"
                                                };
                                                spawn_mini_button(
                                                    row,
                                                    &theme,
                                                    delete_label,
                                                    UiAction::DeleteProfile(profile.id.clone()),
                                                    19 + index as u16 * 2,
                                                );
                                            });
                                    }
                                    let reset_order = 18 + profiles.profiles.len() as u16 * 2;
                                    let reset = if confirmation.action
                                        == Some(ConfirmationAction::ResetStatistics)
                                    {
                                        "CONFIRM RESET"
                                    } else {
                                        "RESET STATS"
                                    };
                                    let all = if confirmation.action
                                        == Some(ConfirmationAction::ResetAllData)
                                    {
                                        "CONFIRM RESET"
                                    } else {
                                        "RESET DATA"
                                    };
                                    spawn_compact_menu_button(
                                        right,
                                        &theme,
                                        reset,
                                        UiAction::ResetStatistics,
                                        reset_order,
                                    );
                                    spawn_compact_menu_button(
                                        right,
                                        &theme,
                                        all,
                                        UiAction::ResetAllData,
                                        reset_order + 1,
                                    );
                                }
                            });
                    });
                if let Some(warning) = &persistence.warning {
                    panel.spawn((
                        Text::new(warning.clone()),
                        TextFont {
                            font: theme.body_font.clone(),
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(Color::srgb(1.0, 0.65, 0.20)),
                    ));
                }
                let back_order = 20 + profiles.profiles.len() as u16 * 2;
                spawn_compact_menu_button(
                    panel,
                    &theme,
                    "BACK",
                    UiAction::SettingsBack,
                    back_order,
                );
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
            BackgroundColor(Color::srgba(0.96, 0.94, 0.88, 0.86)),
        ))
        .with_children(|root| {
            let mut pause_panel = panel_node(percent(90));
            pause_panel.max_width = px(530);
            root.spawn((
                pause_panel,
                BackgroundColor(Color::NONE),
                BorderColor::all(INK),
            ))
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
            BackgroundColor(Color::srgba(0.96, 0.94, 0.88, 0.72)),
        ))
        .with_children(|root| {
            let mut game_over_panel = panel_node(percent(90));
            game_over_panel.max_width = px(620);
            root.spawn((
                game_over_panel,
                BackgroundColor(Color::NONE),
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
            // Keep the table airy enough to scan while retaining all eight
            // competitors and four actions on the 600px stress viewport.
            results_panel.padding = UiRect::axes(px(34), px(8));
            results_panel.row_gap = px(3);
            results_panel.overflow = Overflow::scroll_y();
            root.spawn((
                results_panel,
                BackgroundColor(Color::NONE),
                BorderColor::all(INK),
            ))
            .with_children(|panel| {
                spawn_title(panel, &theme, "FINAL RANKING", 34.0);
                spawn_subtitle(
                    panel,
                    &theme,
                    format!(
                        "{}  /  {:02}:{:02}",
                        if results.winner_name.is_empty() {
                            "NO WINNER"
                        } else {
                            &results.winner_name
                        },
                        (results.duration_seconds / 60.0) as u32,
                        results.duration_seconds as u32 % 60
                    ),
                );
                panel
                    .spawn((Node {
                        width: percent(100),
                        padding: UiRect::axes(px(10), px(2)),
                        ..default()
                    },))
                    .with_children(|header| {
                        header.spawn(result_column("PLACE", px(48), false, &theme));
                        header.spawn(result_column("PLAYER", px(1), true, &theme));
                        header.spawn(result_column("PEAK", px(58), false, &theme));
                        header.spawn(result_column("K/D", px(48), false, &theme));
                        header.spawn(result_column("CAP", px(58), false, &theme));
                        header.spawn(result_column("CELLS", px(58), false, &theme));
                        header.spawn(result_column("TRAIL", px(58), false, &theme));
                    });
                if results.rows.is_empty() {
                    spawn_subtitle(panel, &theme, "NO MATCH DATA");
                }
                for row in &results.rows {
                    panel
                        .spawn((
                            Node {
                                width: percent(100),
                                min_height: px(30),
                                padding: UiRect::axes(px(10), px(2)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(0)),
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            BorderColor::all(Color::srgba(0.75, 0.85, 0.90, 0.16)),
                        ))
                        .with_children(|line| {
                            line.spawn((
                                Node {
                                    width: px(8),
                                    height: px(20),
                                    margin: UiRect::right(px(9)),
                                    border_radius: BorderRadius::all(px(0)),
                                    ..default()
                                },
                                BackgroundColor(palette_color(row.color_id)),
                            ));
                            line.spawn((
                                Text::new(format!("#{:02}", row.placement)),
                                TextFont {
                                    font: theme.body_font.clone(),
                                    font_size: FontSize::Px(13.0),
                                    ..default()
                                },
                                TextColor(if row.placement == 1 { LIME } else { INK }),
                                Node {
                                    width: px(48),
                                    ..default()
                                },
                            ));
                            line.spawn((
                                Text::new(row.name.clone()),
                                TextFont {
                                    font: theme.body_font.clone(),
                                    font_size: FontSize::Px(13.0),
                                    ..default()
                                },
                                TextColor(if row.placement == 1 { LIME } else { INK }),
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                            for value in [
                                format!("{:.0}%", row.peak_percent),
                                format!("{}/{}", row.kills, row.deaths),
                                format!("{:.0}%", row.largest_capture_percent),
                                format!("{}k", (row.total_cells_captured as f32 / 1000.0).round()),
                                format!("{:.0}m", row.longest_trail),
                            ] {
                                line.spawn((
                                    Text::new(value),
                                    TextFont {
                                        font: theme.body_font.clone(),
                                        font_size: FontSize::Px(13.0),
                                        ..default()
                                    },
                                    TextColor(if row.placement == 1 { LIME } else { INK }),
                                    Node {
                                        width: px(58),
                                        ..default()
                                    },
                                ));
                            }
                        });
                }
                spawn_compact_menu_button(
                    panel,
                    &theme,
                    "REMATCH",
                    UiAction::Rematch { same_field: false },
                    0,
                );
                spawn_compact_menu_button(
                    panel,
                    &theme,
                    "REPLAY FIELD",
                    UiAction::Rematch { same_field: true },
                    1,
                );
                spawn_compact_menu_button(
                    panel,
                    &theme,
                    "LOBBY",
                    UiAction::State(AppState::Lobby),
                    2,
                );
                spawn_compact_menu_button(
                    panel,
                    &theme,
                    "HOME",
                    UiAction::State(AppState::Home),
                    3,
                );
            });
        });
}

fn result_column(label: &str, width: Val, grow: bool, theme: &UiTheme) -> impl Bundle {
    (
        Text::new(label),
        TextFont {
            font: theme.body_font.clone(),
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(MUTED),
        Node {
            width,
            flex_grow: if grow { 1.0 } else { 0.0 },
            ..default()
        },
    )
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
                    format!("{value:.1}x")
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
