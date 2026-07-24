//! Live match HUD projection and completed-match persistence bridge.

use super::*;
use crate::render::CompetitorVisual;
use bevy::ecs::system::SystemParam;
use bevy::window::PrimaryWindow;
use std::fmt::Write;

type CompetitorHudQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Competitor,
        &'static LifeState,
        &'static SpawnProtection,
        Option<&'static ActiveTrail>,
    ),
>;

#[derive(SystemParam)]
pub(super) struct HudData<'w, 's> {
    state: Res<'w, State<AppState>>,
    session: Res<'w, MatchSession>,
    rankings: Res<'w, Rankings>,
    eliminations: Option<Res<'w, EliminationFeed>>,
    competitors: CompetitorHudQuery<'w, 's>,
}

type HudTextQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Text,
        Option<&'static GlobalRankingRow>,
        Option<&'static MatchAnnouncementText>,
        Option<&'static KillFeedText>,
        Option<&'static HumanRespawnText>,
        Option<&'static mut TextColor>,
    ),
    Or<(
        With<GlobalRankingRow>,
        With<MatchAnnouncementText>,
        With<KillFeedText>,
        With<HumanRespawnText>,
    )>,
>;

#[derive(SystemParam)]
pub(super) struct ResultsResources<'w> {
    config: Res<'w, GameConfig>,
    session: Res<'w, MatchSession>,
    territory_map: Res<'w, crate::territory_map::TerritoryMap>,
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
            for index in 0..3 {
                root.spawn((
                    Text::new(""),
                    TextFont {
                        font: theme.body_font.clone(),
                        font_size: FontSize::Px(15.0 * text_scale),
                        ..default()
                    },
                    TextColor(MUTED),
                    TextShadow {
                        offset: Vec2::new(1.0, 1.0),
                        color: INK.with_alpha(0.82),
                    },
                    TextLayout::justify(Justify::Right),
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(22),
                        top: px(16 + index as i32 * 27),
                        width: px(236),
                        max_width: px(236),
                        ..default()
                    },
                    GlobalRankingRow(index),
                ));
                root.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(12),
                        top: px(17 + index as i32 * 27),
                        width: px(4),
                        height: px(18),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    GlobalRankingAccent(index),
                ));
            }
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: px(18),
                    top: px(112),
                    width: px(292),
                    min_height: px(42),
                    padding: UiRect::axes(px(14), px(9)),
                    border: UiRect::left(px(4)),
                    border_radius: BorderRadius::all(px(5)),
                    justify_content: JustifyContent::FlexEnd,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(INK.with_alpha(0.90)),
                BorderColor::all(CORAL),
                UiTransform::default(),
                Visibility::Hidden,
                KillFeedPanel,
            ))
            .with_children(|feed| {
                feed.spawn((
                    Text::new(""),
                    TextFont {
                        font: theme.body_font.clone(),
                        font_size: FontSize::Px(13.0 * text_scale),
                        ..default()
                    },
                    TextColor(CREAM),
                    TextShadow {
                        offset: Vec2::new(2.0, 2.0),
                        color: Color::BLACK.with_alpha(0.92),
                    },
                    TextLayout::justify(Justify::Right),
                    Node {
                        width: percent(100),
                        ..default()
                    },
                    KillFeedText,
                ));
                feed.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(7),
                        top: px(7),
                        width: px(5),
                        height: px(5),
                        border_radius: BorderRadius::all(percent(50)),
                        ..default()
                    },
                    BackgroundColor(CORAL),
                    KillFeedAccent,
                ));
            });
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
    tags: Query<(Entity, &HumanNameTag)>,
    visuals: Query<(Entity, &CompetitorVisual, &Competitor)>,
    settings: Res<UserSettings>,
) {
    let Some(theme) = theme else { return };
    let text_scale = if settings.larger_hud_text { 1.22 } else { 1.0 };
    for (root, marker) in &existing {
        if cameras.get(marker.camera).is_err() {
            commands.entity(root).despawn();
        }
    }
    for (tag, marker) in &tags {
        if cameras.get(marker.camera).is_err() || visuals.get(marker.source).is_err() {
            commands.entity(tag).despawn();
        }
    }
    for (camera, player_camera) in &cameras {
        let Some(root_entity) = existing
            .iter()
            .find(|(_, marker)| marker.camera == camera)
            .map(|(root, _)| root)
        else {
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
                        Text::new(""),
                        TextFont {
                            font: theme.display_font.clone(),
                            font_size: FontSize::Px(30.0 * text_scale),
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
                            top: percent(46),
                            left: percent(27),
                            width: percent(46),
                            min_width: px(140),
                            max_width: px(230),
                            ..default()
                        },
                        HumanRespawnText(player_camera.subject),
                    ));
                });
            continue;
        };
        commands.entity(root_entity).with_children(|root| {
            for (source, visual, competitor) in &visuals {
                if tags
                    .iter()
                    .any(|(_, tag)| tag.camera == camera && tag.source == source)
                {
                    continue;
                }
                root.spawn((
                    Name::new("Player Name Tag"),
                    HumanNameTag { camera, source },
                    Text::new(competitor.display_name.clone()),
                    TextFont {
                        font: theme.body_font.clone(),
                        font_size: FontSize::Px(12.0 * text_scale),
                        ..default()
                    },
                    TextColor(palette_color(visual.color_id)),
                    TextShadow {
                        offset: Vec2::new(1.5, 1.5),
                        color: Color::srgba(1.0, 0.97, 0.88, 0.92),
                    },
                    TextLayout::justify(Justify::Center),
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(180),
                        height: px(24),
                        ..default()
                    },
                    Visibility::Hidden,
                ));
            }
        });
    }
}

pub(super) fn update_name_tags(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(Entity, &PlayerCamera, &Camera, &GlobalTransform)>,
    visuals: Query<&CompetitorVisual>,
    mut tags: Query<(&HumanNameTag, &mut Node, &mut Visibility)>,
) {
    let scale_factor = windows.single().map_or(1.0, Window::scale_factor);
    for (marker, mut node, mut visibility) in &mut tags {
        let Some((_, _, camera, camera_transform)) = cameras
            .iter()
            .find(|(camera_entity, _, _, _)| *camera_entity == marker.camera)
        else {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            continue;
        };
        let Ok(visual) = visuals.get(marker.source) else {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            continue;
        };
        if !visual.alive {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            continue;
        }
        let world_position = Vec3::new(visual.position.x, 2.9, visual.position.y);
        let Ok(mut screen_position) = camera.world_to_viewport(camera_transform, world_position)
        else {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            continue;
        };
        if let Some(viewport) = &camera.viewport {
            screen_position -= viewport.physical_position.as_vec2() / scale_factor.max(0.001);
        }
        node.left = px(screen_position.x - 90.0);
        node.top = px(screen_position.y - 32.0);
        if *visibility != Visibility::Inherited {
            *visibility = Visibility::Inherited;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update_gameplay_hud(
    data: HudData,
    time: Res<Time>,
    mut refresh_remaining: Local<f32>,
    mut ranking_lines: Local<[String; 3]>,
    mut announcement_text: Local<String>,
    mut elimination_text: Local<String>,
    mut respawn_text: Local<String>,
    mut texts: HudTextQuery,
    mut accents: Query<(&GlobalRankingAccent, &mut BackgroundColor), Without<KillFeedAccent>>,
    mut kill_feed_panels: Query<(&mut Visibility, &mut BorderColor), With<KillFeedPanel>>,
    mut kill_feed_accents: Query<
        &mut BackgroundColor,
        (With<KillFeedAccent>, Without<GlobalRankingAccent>),
    >,
) {
    let HudData {
        state,
        session,
        rankings,
        eliminations,
        competitors,
    } = data;
    *refresh_remaining -= time.delta_secs();
    if *refresh_remaining > 0.0 {
        return;
    }
    *refresh_remaining = 0.1;

    for line in &mut *ranking_lines {
        line.clear();
    }
    for (index, entry) in rankings.entries.iter().take(3).enumerate() {
        if let Some((competitor, ..)) = competitors
            .iter()
            .find(|(competitor, ..)| competitor.id == entry.id)
        {
            write_ranking_label(
                &mut ranking_lines[index],
                entry.rank,
                &competitor.display_name,
                entry.territory_percent,
            );
        }
    }
    announcement_text.clear();
    match *state.get() {
        AppState::Countdown => {
            let _ = write!(
                announcement_text,
                "{}",
                session.countdown_remaining.ceil().max(1.0) as u8
            );
        }
        AppState::Playing if session.elapsed_seconds < 0.8 => announcement_text.push_str("GO!"),
        AppState::GameOver => {
            if let Some(winner) = session.winner
                && let Some((competitor, ..)) = competitors
                    .iter()
                    .find(|(competitor, ..)| competitor.id == winner)
            {
                let _ = write!(announcement_text, "{} WINS!", competitor.display_name);
            } else {
                announcement_text.push_str("GAME OVER");
            }
        }
        _ => {}
    }
    elimination_text.clear();
    if let Some(feed) = eliminations.as_ref() {
        for entry in feed
            .0
            .iter()
            .rev()
            .filter(|entry| session.elapsed_seconds - entry.match_time <= 5.0)
            .take(4)
        {
            append_elimination_line(&mut elimination_text, entry, &competitors);
        }
    }

    for (mut text, row, announcement, kill_feed, respawn, text_color) in &mut texts {
        if let Some(row) = row {
            update_text(
                &mut text,
                ranking_lines.get(row.0).map_or("", String::as_str),
            );
            if let Some(mut text_color) = text_color {
                let next_color = rankings
                    .entries
                    .get(row.0)
                    .and_then(|entry| {
                        competitors
                            .iter()
                            .find(|(competitor, ..)| competitor.id == entry.id)
                            .map(|(competitor, ..)| palette_color(competitor.color_id))
                    })
                    .unwrap_or(Color::NONE);
                set_text_color_if_changed(&mut text_color, next_color);
            }
        } else if announcement.is_some() {
            update_text(&mut text, &announcement_text);
        } else if kill_feed.is_some() {
            update_text(&mut text, &elimination_text);
        } else if let Some(marker) = respawn {
            let Ok((_, life, protection, _)) = competitors.get(marker.0) else {
                continue;
            };
            respawn_text.clear();
            if !life.is_alive() {
                let _ = write!(
                    respawn_text,
                    "RESPAWN\n{:.1}",
                    life.respawn_remaining.max(0.0)
                );
            } else if protection.active() {
                let _ = write!(respawn_text, "SHIELD\n{:.1}", protection.remaining);
            }
            update_text(&mut text, &respawn_text);
            if let Some(mut text_color) = text_color {
                set_text_color_if_changed(
                    &mut text_color,
                    if respawn_text.is_empty() {
                        Color::NONE
                    } else {
                        CORAL
                    },
                );
            }
        }
    }
    for (accent, mut background) in &mut accents {
        let next_color = rankings
            .entries
            .get(accent.0)
            .and_then(|entry| {
                competitors
                    .iter()
                    .find(|(competitor, ..)| competitor.id == entry.id)
                    .map(|(competitor, ..)| palette_color(competitor.color_id).with_alpha(0.86))
            })
            .unwrap_or(Color::NONE);
        if background.0 != next_color {
            background.0 = next_color;
        }
    }
    let event_color = eliminations
        .as_ref()
        .and_then(|feed| feed.0.last())
        .and_then(|entry| entry.killer.or(Some(entry.victim)))
        .and_then(|id| {
            competitors
                .iter()
                .find(|(competitor, ..)| competitor.id == id)
                .map(|(competitor, ..)| palette_color(competitor.color_id))
        })
        .unwrap_or(CORAL);
    for (mut visibility, mut border) in &mut kill_feed_panels {
        *visibility = if elimination_text.is_empty() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        border.set_all(event_color);
    }
    for mut accent in &mut kill_feed_accents {
        accent.0 = event_color;
    }
}

pub(super) fn animate_kill_feed(
    time: Res<Time>,
    session: Res<MatchSession>,
    settings: Res<UserSettings>,
    eliminations: Option<Res<EliminationFeed>>,
    mut panels: Query<(&mut UiTransform, &mut BackgroundColor), With<KillFeedPanel>>,
) {
    let age = eliminations
        .as_ref()
        .and_then(|feed| feed.0.last())
        .map_or(f32::INFINITY, |event| {
            (session.elapsed_seconds - event.match_time).max(0.0)
        });
    let reduced_motion = settings.reduced_motion;
    for (mut transform, mut background) in &mut panels {
        let (translation, scale) = kill_feed_transform(age, reduced_motion);
        transform.translation = Val2::px(translation, 0.0);
        transform.scale = Vec2::splat(scale);
        let progress = (age / 0.55).clamp(0.0, 1.0);
        let shimmer = if reduced_motion || !age.is_finite() {
            0.0
        } else {
            (time.elapsed_secs() * 8.0).sin().max(0.0) * (1.0 - progress)
        };
        background.0 = INK.with_alpha(0.90 + shimmer * 0.08);
    }
}

fn kill_feed_transform(age: f32, reduced_motion: bool) -> (f32, f32) {
    if reduced_motion {
        return (0.0, 1.0);
    }
    let progress = (age / 0.55).clamp(0.0, 1.0);
    let eased = 1.0 - (1.0 - progress).powi(3);
    let bounce = (progress * std::f32::consts::PI).sin() * (1.0 - progress) * 0.09;
    (30.0 * (1.0 - eased), 0.94 + eased * 0.06 + bounce)
}

pub(super) fn write_ranking_label(target: &mut String, rank: u8, name: &str, percent: f32) {
    let _ = write!(target, "{rank}  {name}   {percent:.1}%");
}

fn update_text(text: &mut Text, next: &str) {
    if text.0 != next {
        text.0.clear();
        text.0.push_str(next);
    }
}

fn set_text_color_if_changed(text_color: &mut TextColor, next: Color) {
    if text_color.0 != next {
        text_color.0 = next;
    }
}

fn append_elimination_line(
    target: &mut String,
    entry: &crate::match_game::EliminationRecord,
    competitors: &CompetitorHudQuery,
) {
    let Some((victim, ..)) = competitors
        .iter()
        .find(|(competitor, ..)| competitor.id == entry.victim)
    else {
        return;
    };
    if !target.is_empty() {
        target.push('\n');
    }
    let killer = entry.killer.and_then(|id| {
        competitors
            .iter()
            .find(|(competitor, ..)| competitor.id == id)
            .map(|(competitor, ..)| competitor.display_name.as_str())
    });
    match (entry.cause, killer) {
        (DeathCause::SelfTrail, _) => {
            let _ = write!(target, "SELF CUT!  {}", victim.display_name);
        }
        (DeathCause::Displaced, Some(killer)) => {
            let _ = write!(target, "ERASED!  {killer}  >  {}", victim.display_name);
        }
        (_, Some(killer)) => {
            let _ = write!(target, "CUT!  {killer}  >  {}", victim.display_name);
        }
        _ => {
            let _ = write!(target, "WIPED OUT!  {}", victim.display_name);
        }
    }
}

pub(super) fn collect_match_results(
    competitors: Query<(&Competitor, &SimulationMatchStatistics)>,
    resources: ResultsResources,
) {
    let ResultsResources {
        config,
        session,
        territory_map,
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
        b.peak_territory_area
            .total_cmp(&a.peak_territory_area)
            .then_with(|| b.peak_territory_cells.cmp(&a.peak_territory_cells))
            .then_with(|| b.kills.cmp(&a.kills))
            .then_with(|| a.deaths.cmp(&b.deaths))
            .then_with(|| a_competitor.id.cmp(&b_competitor.id))
    });
    let percent = |area: f32| {
        if territory_map.arena_area <= f32::EPSILON {
            0.0
        } else {
            config.display_territory_percent(area.max(0.0) * 100.0 / territory_map.arena_area)
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
            peak_percent: percent(stats.peak_territory_area),
            kills: stats.kills,
            deaths: stats.deaths,
            largest_capture_percent: percent(stats.largest_capture_area),
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
            area_captured_total: stats.area_captured_total,
            area_stolen_total: stats.area_stolen_total,
            largest_capture_area: stats.largest_capture_area,
            peak_territory_area: stats.peak_territory_area,
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
            territory_map.arena_area,
        );
    }
    persisted.0 = Some(session.seed);
    persistence.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::kill_feed_transform;

    #[test]
    fn kill_feed_enters_with_a_bounce_then_settles() {
        let start = kill_feed_transform(0.0, false);
        let middle = kill_feed_transform(0.25, false);
        let settled = kill_feed_transform(0.55, false);

        assert_eq!(start, (30.0, 0.94));
        assert!(middle.0 > 0.0 && middle.0 < start.0);
        assert!(middle.1 > 1.0);
        assert_eq!(settled, (0.0, 1.0));
    }

    #[test]
    fn reduced_motion_shows_the_kill_feed_at_rest_immediately() {
        assert_eq!(kill_feed_transform(0.0, true), (0.0, 1.0));
    }
}
