//! Local profile leaderboard.

use super::super::*;

pub(in crate::ui) fn spawn_leaderboard(
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
                let leaderboard = profiles.sorted_leaderboard();
                if leaderboard.is_empty() {
                    spawn_subtitle(panel, &theme, "NO MATCHES YET");
                } else {
                    panel
                        .spawn((Node {
                            width: percent(100),
                            padding: UiRect::axes(px(10), px(2)),
                            column_gap: px(8),
                            ..default()
                        },))
                        .with_children(|header| {
                            header.spawn((Node {
                                width: px(4),
                                ..default()
                            },));
                            for (label, width, grow) in [
                                ("RANK", 52.0, false),
                                ("PLAYER", 1.0, true),
                                ("WINS", 76.0, false),
                                ("RATE", 58.0, false),
                                ("KILLS", 58.0, false),
                                ("BEST TURF", 92.0, false),
                            ] {
                                header.spawn((
                                    Text::new(label),
                                    TextFont {
                                        font: theme.body_font.clone(),
                                        font_size: FontSize::Px(11.0),
                                        ..default()
                                    },
                                    TextColor(MUTED),
                                    Node {
                                        width: px(width),
                                        flex_grow: if grow { 1.0 } else { 0.0 },
                                        ..default()
                                    },
                                ));
                            }
                        });
                }
                for (rank, profile) in leaderboard.into_iter().enumerate() {
                    let stats = &profile.statistics;
                    let win_rate = if stats.games_played == 0 {
                        0.0
                    } else {
                        stats.wins as f32 * 100.0 / stats.games_played as f32
                    };
                    let color = palette_color(rank as u8);
                    panel
                        .spawn((
                            Node {
                                width: percent(100),
                                min_height: px(38),
                                padding: UiRect::axes(px(10), px(6)),
                                column_gap: px(8),
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(if rank == 0 {
                                color.with_alpha(0.055)
                            } else {
                                Color::NONE
                            }),
                        ))
                        .with_children(|row| {
                            row.spawn((
                                Node {
                                    width: px(4),
                                    height: px(22),
                                    ..default()
                                },
                                BackgroundColor(color),
                            ));
                            row.spawn((
                                Text::new(format!("#{:02}", rank + 1)),
                                TextFont {
                                    font: theme.body_font.clone(),
                                    font_size: FontSize::Px(18.0),
                                    ..default()
                                },
                                TextColor(color),
                                Node {
                                    width: px(52),
                                    ..default()
                                },
                            ));
                            row.spawn((
                                Text::new(profile.display_name.clone()),
                                TextFont {
                                    font: theme.body_font.clone(),
                                    font_size: FontSize::Px(17.0),
                                    ..default()
                                },
                                TextColor(color),
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                            for (value, width) in [
                                (format!("{}/{}", stats.wins, stats.games_played), 76.0),
                                (format!("{win_rate:.0}%"), 58.0),
                                (stats.kills.to_string(), 58.0),
                                (format!("{:.0}%", stats.best_territory_percent), 92.0),
                            ] {
                                row.spawn((
                                    Text::new(value),
                                    TextFont {
                                        font: theme.body_font.clone(),
                                        font_size: FontSize::Px(16.0),
                                        ..default()
                                    },
                                    TextColor(color),
                                    Node {
                                        width: px(width),
                                        ..default()
                                    },
                                ));
                            }
                        });
                }
                spawn_button(panel, &theme, "BACK", UiAction::Back(AppState::Home), 0);
            });
        });
}
