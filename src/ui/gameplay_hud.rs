//! Live match HUD projection and completed-match persistence bridge.

use super::*;
use bevy::ecs::system::SystemParam;

type CompetitorHudQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Competitor,
        &'static TerritoryRecord,
        &'static LifeState,
        &'static SpawnProtection,
        Option<&'static ActiveTrail>,
    ),
>;

#[derive(SystemParam)]
pub(super) struct HudData<'w, 's> {
    state: Res<'w, State<AppState>>,
    session: Res<'w, MatchSession>,
    board: Res<'w, BoardGrid>,
    rankings: Res<'w, Rankings>,
    eliminations: Option<Res<'w, EliminationFeed>>,
    competitors: CompetitorHudQuery<'w, 's>,
}

type HudTextQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Text,
        Option<&'static GlobalRankingText>,
        Option<&'static MatchAnnouncementText>,
        Option<&'static KillFeedText>,
        Option<&'static HumanHudSummary>,
        Option<&'static HumanHudRank>,
        Option<&'static HumanRespawnText>,
        Option<&'static mut BackgroundColor>,
    ),
    Or<(
        With<GlobalRankingText>,
        With<MatchAnnouncementText>,
        With<KillFeedText>,
        With<HumanHudSummary>,
        With<HumanHudRank>,
        With<HumanRespawnText>,
    )>,
>;

#[derive(SystemParam)]
pub(super) struct ResultsResources<'w> {
    session: Res<'w, MatchSession>,
    board: Res<'w, BoardGrid>,
    setup: Res<'w, MatchSetup>,
    results: ResMut<'w, MatchResults>,
    profiles: ResMut<'w, ProfileStore>,
    persisted: ResMut<'w, PersistedMatchSeed>,
    persistence: ResMut<'w, PersistenceStatus>,
}
pub(super) fn spawn_gameplay_hud(
    mut commands: Commands,
    theme: Res<UiTheme>,
    settings: Res<UserSettings>,
    existing: Query<Entity, With<GameplayHudRoot>>,
) {
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let text_scale = if settings.larger_hud_text { 1.22 } else { 1.0 };
    commands
        .spawn((
            GameplayHudRoot,
            Node {
                width: percent(100),
                height: percent(100),
                position_type: PositionType::Absolute,
                ..default()
            },
            GlobalZIndex(60),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(50),
                    top: px(104),
                    width: px(220),
                    max_width: px(220),
                    margin: UiRect::left(px(-110)),
                    padding: UiRect::axes(px(10), px(8)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.035, 0.055, 0.09, 0.94)),
                BorderColor::all(Color::srgba(0.90, 0.93, 0.88, 0.20)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("LEADERS"),
                    TextFont {
                        font: theme.body_font.clone(),
                        font_size: FontSize::Px(15.0 * text_scale),
                        ..default()
                    },
                    TextColor(CREAM),
                    Node {
                        width: percent(100),
                        ..default()
                    },
                    GlobalRankingText,
                ));
            });
            root.spawn((
                Text::new(""),
                TextFont {
                    font: theme.body_font.clone(),
                    font_size: FontSize::Px(13.0 * text_scale),
                    ..default()
                },
                TextColor(CREAM),
                TextLayout::justify(Justify::Right),
                Node {
                    position_type: PositionType::Absolute,
                    right: px(18),
                    top: px(196),
                    width: px(280),
                    padding: UiRect::axes(px(10), px(7)),
                    max_width: px(280),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.035, 0.055, 0.09, 0.0)),
                BorderColor::all(Color::srgba(0.90, 0.93, 0.88, 0.0)),
                KillFeedText,
            ));
            root.spawn((
                Text::new("3"),
                TextFont {
                    font: theme.body_font.clone(),
                    font_size: FontSize::Px(72.0),
                    ..default()
                },
                TextColor(CORAL),
                TextShadow {
                    offset: Vec2::new(6.0, 7.0),
                    color: INK,
                },
                TextLayout::justify(Justify::Center),
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    top: percent(40),
                    ..default()
                },
                MatchAnnouncementText,
            ));
        });
}

pub(super) fn cleanup_gameplay_hud(
    mut commands: Commands,
    global: Query<Entity, With<GameplayHudRoot>>,
    local: Query<Entity, With<HumanHudRoot>>,
) {
    for entity in &global {
        commands.entity(entity).despawn();
    }
    for entity in &local {
        commands.entity(entity).despawn();
    }
}

pub(super) fn reconcile_human_huds(
    mut commands: Commands,
    theme: Option<Res<UiTheme>>,
    cameras: Query<(Entity, &PlayerCamera)>,
    existing: Query<(Entity, &HumanHudRoot)>,
    settings: Res<UserSettings>,
) {
    let Some(theme) = theme else { return };
    let text_scale = if settings.larger_hud_text { 1.22 } else { 1.0 };
    for (root, marker) in &existing {
        if cameras.get(marker.camera).is_err() {
            commands.entity(root).despawn();
        }
    }
    for (camera, player_camera) in &cameras {
        if existing.iter().any(|(_, marker)| marker.camera == camera) {
            continue;
        }
        commands
            .spawn((
                HumanHudRoot { camera },
                UiTargetCamera(camera),
                Node {
                    width: percent(100),
                    height: percent(100),
                    position_type: PositionType::Absolute,
                    ..default()
                },
                GlobalZIndex(50),
                Pickable::IGNORE,
            ))
            .with_children(|root| {
                root.spawn((
                    Text::new("PLAYER"),
                    TextFont {
                        font: theme.body_font.clone(),
                        font_size: FontSize::Px(15.0 * text_scale),
                        ..default()
                    },
                    TextColor(CREAM),
                    TextShadow {
                        offset: Vec2::new(2.0, 2.0),
                        color: INK,
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(24),
                        left: px(12),
                        width: percent(90),
                        max_width: px(380),
                        min_width: px(180),
                        overflow: Overflow::clip(),
                        padding: UiRect::axes(px(9), px(7)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.035, 0.055, 0.09, 0.90)),
                    BorderColor::all(Color::srgba(0.90, 0.93, 0.88, 0.20)),
                    HumanHudSummary(player_camera.subject),
                ));
                root.spawn((
                    Text::new("#1 / 8"),
                    TextFont {
                        font: theme.body_font.clone(),
                        font_size: FontSize::Px(15.0 * text_scale),
                        ..default()
                    },
                    TextColor(CREAM),
                    TextShadow {
                        offset: Vec2::new(2.0, 2.0),
                        color: INK,
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: px(22),
                        left: px(12),
                        width: px(88),
                        overflow: Overflow::clip(),
                        padding: UiRect::axes(px(9), px(7)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.035, 0.055, 0.09, 0.90)),
                    BorderColor::all(Color::srgba(0.90, 0.93, 0.88, 0.20)),
                    HumanHudRank(player_camera.subject),
                ));
                root.spawn((
                    Text::new(""),
                    TextFont {
                        font: theme.display_font.clone(),
                        font_size: FontSize::Px(29.0 * text_scale),
                        ..default()
                    },
                    TextColor(CORAL),
                    TextShadow {
                        offset: Vec2::new(3.0, 3.0),
                        color: INK,
                    },
                    TextLayout::justify(Justify::Center),
                    Node {
                        position_type: PositionType::Absolute,
                        top: percent(44),
                        left: percent(25),
                        width: percent(50),
                        min_width: px(160),
                        max_width: px(260),
                        padding: UiRect::axes(px(14), px(8)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.035, 0.055, 0.09, 0.0)),
                    BorderColor::all(Color::srgba(0.90, 0.93, 0.88, 0.0)),
                    HumanRespawnText(player_camera.subject),
                ));
            });
    }
}

pub(super) fn update_gameplay_hud(data: HudData, mut texts: HudTextQuery) {
    let HudData {
        state,
        session,
        board,
        rankings,
        eliminations,
        competitors,
    } = data;
    let mut lines = vec!["RACE ORDER".to_owned()];
    for entry in rankings.entries.iter().take(3) {
        if let Some((competitor, ..)) = competitors
            .iter()
            .find(|(competitor, ..)| competitor.id == entry.id)
        {
            let npc = if competitor.kind == CompetitorKind::Npc {
                " [NPC]"
            } else {
                ""
            };
            lines.push(format!(
                "{}  {}{}  {:>5.1}%",
                entry.rank, competitor.display_name, npc, entry.territory_percent
            ));
        }
    }
    let ranking_text = lines.join("\n");
    let announcement_text = match *state.get() {
        AppState::Countdown => format!("{}", session.countdown_remaining.ceil().max(1.0) as u8),
        AppState::Playing if session.elapsed_seconds < 0.8 => "GO!".to_owned(),
        AppState::GameOver => session
            .winner
            .and_then(|winner| {
                competitors
                    .iter()
                    .find(|(competitor, ..)| competitor.id == winner)
                    .map(|(competitor, ..)| format!("{} WINS!", competitor.display_name))
            })
            .unwrap_or_else(|| "GAME OVER".to_owned()),
        _ => String::new(),
    };
    let elimination_text = eliminations.as_ref().map_or_else(String::new, |feed| {
        feed.0
            .iter()
            .rev()
            .filter(|entry| session.elapsed_seconds - entry.match_time <= 5.0)
            .take(4)
            .filter_map(|entry| elimination_line(entry, &competitors))
            .collect::<Vec<_>>()
            .join("\n")
    });

    for (mut text, global, announcement, kill_feed, summary, rank, respawn, background) in
        &mut texts
    {
        if global.is_some() {
            text.0.clone_from(&ranking_text);
        } else if announcement.is_some() {
            text.0.clone_from(&announcement_text);
        } else if kill_feed.is_some() {
            text.0.clone_from(&elimination_text);
            if let Some(mut background) = background {
                background.0 = Color::srgba(
                    0.035,
                    0.055,
                    0.09,
                    if text.0.is_empty() { 0.0 } else { 0.90 },
                );
            }
        } else if let Some(marker) = summary {
            let Ok((competitor, territory, life, protection, trail)) = competitors.get(marker.0)
            else {
                continue;
            };
            let percent = if board.playable_cells == 0 {
                0.0
            } else {
                territory.current_cells as f32 * 100.0 / board.playable_cells as f32
            };
            let status = if !life.is_alive() {
                "  /  RESPAWNING"
            } else if protection.active() {
                "  /  SHIELDED"
            } else if trail.is_some() {
                "  /  TRAIL ACTIVE"
            } else {
                ""
            };
            text.0 = format!("{}  {:.1}%{}", competitor.display_name, percent, status);
        } else if let Some(marker) = rank {
            let Ok((competitor, ..)) = competitors.get(marker.0) else {
                continue;
            };
            let rank = rankings
                .rank_of(competitor.id)
                .map_or(0, |entry| entry.rank);
            text.0 = format!("#{rank} / {}", rankings.entries.len());
        } else if let Some(marker) = respawn {
            let Ok((_, _, life, protection, _)) = competitors.get(marker.0) else {
                continue;
            };
            text.0 = if !life.is_alive() {
                format!("RESPAWN\n{:.1}", life.respawn_remaining.max(0.0))
            } else if protection.active() {
                format!("SHIELD {:.1}", protection.remaining)
            } else {
                String::new()
            };
            if let Some(mut background) = background {
                background.0 = Color::srgba(
                    0.035,
                    0.055,
                    0.09,
                    if text.0.is_empty() { 0.0 } else { 0.90 },
                );
            }
        }
    }
}

fn elimination_line(
    entry: &crate::match_game::EliminationRecord,
    competitors: &CompetitorHudQuery,
) -> Option<String> {
    let victim = competitors
        .iter()
        .find(|(competitor, ..)| competitor.id == entry.victim)?
        .0;
    let killer = entry.killer.and_then(|id| {
        competitors
            .iter()
            .find(|(competitor, ..)| competitor.id == id)
            .map(|(competitor, ..)| competitor.display_name.as_str())
    });
    Some(match (entry.cause, killer) {
        (DeathCause::SelfTrail, _) => format!("{} CUT THEMSELVES", victim.display_name),
        (DeathCause::Displaced, Some(killer)) => {
            format!("{killer} ERASED {}", victim.display_name)
        }
        (_, Some(killer)) => format!("{killer} CUT {}", victim.display_name),
        _ => format!("{} WIPED OUT", victim.display_name),
    })
}

pub(super) fn collect_match_results(
    competitors: Query<(&Competitor, &SimulationMatchStatistics)>,
    resources: ResultsResources,
) {
    let ResultsResources {
        session,
        board,
        setup,
        mut results,
        mut profiles,
        mut persisted,
        mut persistence,
    } = resources;
    let mut rows: Vec<_> = competitors
        .iter()
        .map(|(competitor, stats)| (competitor, *stats))
        .collect();
    rows.sort_by(|(a_competitor, a), (b_competitor, b)| {
        b.peak_territory_cells
            .cmp(&a.peak_territory_cells)
            .then_with(|| b.kills.cmp(&a.kills))
            .then_with(|| a.deaths.cmp(&b.deaths))
            .then_with(|| a_competitor.id.cmp(&b_competitor.id))
    });
    let percent = |cells: u32| {
        if board.playable_cells == 0 {
            0.0
        } else {
            cells as f32 * 100.0 / board.playable_cells as f32
        }
    };
    results.winner_name = session
        .winner
        .and_then(|winner| {
            competitors
                .iter()
                .find(|(competitor, _)| competitor.id == winner)
                .map(|(competitor, _)| competitor.display_name.clone())
        })
        .unwrap_or_default();
    results.duration_seconds = session.elapsed_seconds;
    results.rows = rows
        .iter()
        .enumerate()
        .map(|(index, (competitor, stats))| ResultRow {
            name: competitor.display_name.clone(),
            color_id: competitor.color_id,
            placement: index as u8 + 1,
            peak_percent: percent(stats.peak_territory_cells),
            kills: stats.kills,
            deaths: stats.deaths,
            largest_capture_percent: percent(stats.largest_capture_cells),
            total_cells_captured: stats.cells_captured_total,
            longest_trail: stats.longest_trail_length,
        })
        .collect();

    if persisted.0 == Some(session.seed) {
        return;
    }
    for (competitor, stats) in rows {
        if competitor.kind != CompetitorKind::Human {
            continue;
        }
        let Some(human) = setup.humans.get(competitor.id.index()) else {
            continue;
        };
        let Some(profile_id) = human.profile_id.as_deref() else {
            continue;
        };
        let Some(profile) = profiles
            .profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
        else {
            continue;
        };
        let profile_stats = ProfileMatchStatistics {
            kills: stats.kills,
            deaths: stats.deaths,
            captures_completed: stats.captures_completed,
            cells_captured_total: stats.cells_captured_total,
            cells_stolen_total: stats.cells_stolen_total,
            largest_capture_cells: stats.largest_capture_cells,
            peak_territory_cells: stats.peak_territory_cells,
            longest_trail_length: stats.longest_trail_length,
            time_alive_seconds: stats.time_alive_seconds,
        };
        apply_match_statistics(
            &mut profile.statistics,
            &profile_stats,
            session.winner == Some(competitor.id),
            board.playable_cells,
        );
    }
    persisted.0 = Some(session.seed);
    persistence.dirty = true;
}
