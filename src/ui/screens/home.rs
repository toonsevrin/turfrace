//! Home shell and match-loading presentation.

use super::super::*;

pub(in crate::ui) fn spawn_home(mut commands: Commands, theme: Res<UiTheme>) {
    commands
        .spawn((ScreenRoot, screen_node(), BackgroundColor(Color::NONE)))
        .with_children(|root| {
            let mut home_panel = panel_node(percent(52));
            home_panel.max_width = px(580);
            home_panel.padding = UiRect::axes(px(28), px(24));
            root.spawn((
                home_panel,
                BackgroundColor(Color::NONE),
                BorderColor::all(Color::NONE),
            ))
            .with_children(|panel| {
                spawn_title(panel, &theme, "TURFRACE", 50.0);
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
