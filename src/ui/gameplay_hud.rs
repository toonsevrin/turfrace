//! Live match HUD projection and completed-match persistence bridge.

use super::*;
use crate::render::{CompetitorProxy, CompetitorVisual};
use bevy::ecs::system::SystemParam;
use std::fmt::Write;

#[cfg(debug_assertions)]
#[derive(Resource, Default)]
pub(super) struct NpcOverlayState {
    visible: bool,
}

#[cfg(debug_assertions)]
#[derive(Component)]
pub(super) struct NpcDebugOverlay;

#[cfg(debug_assertions)]
#[allow(clippy::too_many_arguments)]
pub(super) fn update_npc_debug_overlay(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    state: Res<State<AppState>>,
    theme: Res<UiTheme>,
    mut overlay_state: ResMut<NpcOverlayState>,
    overlays: Query<Entity, With<NpcDebugOverlay>>,
    mut overlay_text: Query<&mut Text, With<NpcDebugOverlay>>,
    npcs: Query<(&Competitor, &crate::npc::NpcController)>,
) {
    if keyboard.just_pressed(KeyCode::F8) {
        overlay_state.visible = !overlay_state.visible;
    }
    if !overlay_state.visible || !matches!(*state.get(), AppState::Playing | AppState::Countdown) {
        for entity in &overlays {
            commands.entity(entity).despawn();
        }
        return;
    }
    let mut content = String::from("NPC DEBUG [F8]\n");
    for (competitor, controller) in &npcs {
        let observed = controller
            .memory
            .opponents
            .iter()
            .filter(|estimate| estimate.observations > 0)
            .count();
        let _ = writeln!(
            content,
            "{} {:?} a={:?} risk={:.2} area={:.1} r={:.1} safe={} wp={}/{} opp={} traits s{:.2} a{:.2} g{:.2} e{:.2} c{:.2} ad{:.2} co{:.2} t{:.2}",
            competitor.display_name,
            controller.brain_kind,
            controller.last_decision.action,
            controller.last_decision.risk_budget,
            controller.memory.planned_capture_area,
            controller.traits.perception_radius(),
            controller.safety_override,
            controller.memory.waypoint_index,
            controller.memory.waypoint_count,
            observed,
            controller.traits.skill,
            controller.traits.aggression,
            controller.traits.greed,
            controller.traits.exploration,
            controller.traits.composure,
            controller.traits.adaptability,
            controller.traits.commitment,
            controller.traits.turning_bias,
        );
    }
    if let Ok(mut text) = overlay_text.single_mut() {
        if text.0 != content {
            text.0 = content;
        }
        return;
    }
    commands.spawn((
        NpcDebugOverlay,
        Text::new(content),
        TextFont {
            font: theme.body_font.clone(),
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            left: px(8),
            top: px(8),
            max_width: percent(92),
            ..default()
        },
        GlobalZIndex(200),
        Pickable::IGNORE,
    ));
}

type CompetitorHudQuery<'w, 's> = Query<'w, 's, (&'static Competitor, &'static LifeState)>;

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
pub(super) fn gameplay_hud_is_missing(existing: Query<(), With<GameplayHudRoot>>) -> bool {
    existing.is_empty()
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
                    top: px(104),
                    width: px(258),
                    min_height: px(34),
                    padding: UiRect::axes(px(10), px(6)),
                    border: UiRect::left(px(4)),
                    border_radius: BorderRadius::all(px(5)),
                    justify_content: JustifyContent::FlexEnd,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(CORAL.with_alpha(0.78)),
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
                    TextColor(INK),
                    TextShadow {
                        offset: Vec2::new(1.0, 1.0),
                        color: PAPER.with_alpha(0.90),
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
                    if let Ok((_, _, competitor)) = visuals.get(player_camera.subject) {
                        spawn_identity(root, &theme, player_camera.subject, competitor, text_scale);
                    }
                    root.spawn((
                        Text::new(""),
                        TextFont {
                            font: theme.display_font.clone(),
                            font_size: FontSize::Px(22.0 * text_scale),
                            ..default()
                        },
                        TextColor(CORAL),
                        TextShadow {
                            offset: Vec2::new(2.0, 2.0),
                            color: INK,
                        },
                        TextLayout::justify(Justify::Center),
                        Node {
                            position_type: PositionType::Absolute,
                            // Keep the countdown below the centered cube
                            // even when the respawn camera has eased low.
                            top: percent(68),
                            left: percent(22),
                            width: percent(56),
                            min_width: px(140),
                            ..default()
                        },
                        HumanRespawnText(player_camera.subject),
                    ));
                });
            continue;
        };
        commands.entity(root_entity).with_children(|root| {
            for (source, _, competitor) in &visuals {
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
                        font_size: FontSize::Px(11.0 * text_scale),
                        ..default()
                    },
                    TextColor(INK),
                    TextShadow {
                        offset: Vec2::new(1.0, 1.0),
                        color: PAPER.with_alpha(0.92),
                    },
                    TextLayout::justify(Justify::Center),
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(NAME_TAG_WIDTH),
                        height: px(22),
                        ..default()
                    },
                    // Projected labels should retain the camera's subpixel motion.
                    // UI pixel rounding otherwise makes them step independently of
                    // the continuously rendered cube underneath.
                    LayoutConfig {
                        use_rounding: false,
                    },
                    Visibility::Hidden,
                ));
            }
        });
    }
}

const NAME_TAG_WIDTH: f32 = 180.0;
const NAME_TAG_VERTICAL_OFFSET: f32 = 32.0;

pub(super) fn update_name_tags(
    ui_scale: Res<UiScale>,
    cameras: Query<(&Camera, &Transform), With<PlayerCamera>>,
    visuals: Query<&CompetitorVisual>,
    proxies: Query<(&CompetitorProxy, &Transform)>,
    mut tags: Query<(&HumanNameTag, &mut Node, &mut Visibility)>,
) {
    for (marker, mut node, mut visibility) in &mut tags {
        let Ok((camera, camera_transform)) = cameras.get(marker.camera) else {
            hide_name_tag(&mut visibility);
            continue;
        };
        let Ok(visual) = visuals.get(marker.source) else {
            hide_name_tag(&mut visibility);
            continue;
        };
        if !visual.alive {
            hide_name_tag(&mut visibility);
            continue;
        }
        let Some((_, competitor_transform)) = proxies
            .iter()
            .find(|(proxy, _)| proxy.source == marker.source)
        else {
            hide_name_tag(&mut visibility);
            continue;
        };
        // Track the interpolated render proxy, not the fixed-step simulation
        // snapshot. Otherwise the text advances in 60 Hz steps while the cube
        // eases continuously underneath it.
        let mut world_position = competitor_transform.translation;
        world_position.y += 2.9;
        // Player cameras are roots, so their current local transform is also
        // their world transform. Construct it directly because ordinary
        // Transform propagation has not run yet in PostUpdate.
        let camera_transform = GlobalTransform::from(*camera_transform);
        let Ok(screen_position) = camera.world_to_viewport(&camera_transform, world_position)
        else {
            hide_name_tag(&mut visibility);
            continue;
        };
        let Some(viewport) = camera.logical_viewport_rect() else {
            hide_name_tag(&mut visibility);
            continue;
        };
        // Do not place a label that cannot be seen by this split-screen camera.
        // In particular, this avoids changing hidden labels' layout every frame.
        if !viewport.contains(screen_position) {
            hide_name_tag(&mut visibility);
            continue;
        }
        // Camera projection is target-logical, while this camera-local UI root
        // uses scaled logical pixels.
        let ui_position = name_tag_ui_position(screen_position, viewport.min, ui_scale.0);
        set_name_tag_layout(&mut node, &mut visibility, ui_position);
    }
}

fn name_tag_ui_position(screen_position: Vec2, viewport_position: Vec2, ui_scale: f32) -> Vec2 {
    (screen_position - viewport_position) / ui_scale.max(0.001)
}

fn hide_name_tag(visibility: &mut Visibility) {
    if *visibility != Visibility::Hidden {
        *visibility = Visibility::Hidden;
    }
}

fn set_name_tag_layout(node: &mut Node, visibility: &mut Visibility, position: Vec2) -> bool {
    let left = px(position.x - NAME_TAG_WIDTH * 0.5);
    let top = px(position.y - NAME_TAG_VERTICAL_OFFSET);
    let mut changed = false;
    if node.left != left {
        node.left = left;
        changed = true;
    }
    if node.top != top {
        node.top = top;
        changed = true;
    }
    if *visibility != Visibility::Inherited {
        *visibility = Visibility::Inherited;
        changed = true;
    }
    changed
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
            let Ok((_, life)) = competitors.get(marker.0) else {
                continue;
            };
            write_respawn_status(&mut respawn_text, *life);
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
    _time: Res<Time>,
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
        set_kill_feed_style(&mut transform, &mut background, age, reduced_motion);
    }
}

fn set_kill_feed_style(
    transform: &mut UiTransform,
    background: &mut BackgroundColor,
    age: f32,
    reduced_motion: bool,
) -> bool {
    let (translation, scale) = kill_feed_transform(age, reduced_motion);
    let next_translation = Val2::px(translation, 0.0);
    let next_scale = Vec2::splat(scale);
    let mut changed = false;
    if transform.translation != next_translation {
        transform.translation = next_translation;
        changed = true;
    }
    if transform.scale != next_scale {
        transform.scale = next_scale;
        changed = true;
    }
    // The rail and text carry the event; a dark block should not compete
    // with the leaderboard or active trails.
    if background.0 != Color::NONE {
        background.0 = Color::NONE;
        changed = true;
    }
    changed
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

fn write_respawn_status(target: &mut String, life: LifeState) {
    target.clear();
    if !life.is_alive() {
        let _ = write!(target, "RESPAWN\n{:.1}", life.respawn_remaining.max(0.0));
    }
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
            .then_with(|| b.kills.cmp(&a.kills))
            .then_with(|| a.deaths.cmp(&b.deaths))
            .then_with(|| a_competitor.id.cmp(&b_competitor.id))
    });
    let arena_area = territory_map.arena_area();
    let percent = |area: f32| {
        if arena_area <= f32::EPSILON {
            0.0
        } else {
            config.display_territory_percent(area.max(0.0) * 100.0 / arena_area)
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
            longest_trail: stats.longest_trail_length,
            total_captured_area: stats.area_captured_total,
        })
        .collect();

    if persisted.0 == Some(session.field_seed) {
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
            longest_trail_length: stats.longest_trail_length,
            time_alive_seconds: stats.time_alive_seconds,
        };
        apply_match_statistics(
            &mut profile.statistics,
            &profile_stats,
            session.winner == Some(competitor.id),
            arena_area,
        );
    }
    persisted.0 = Some(session.field_seed);
    persistence.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::{
        kill_feed_transform, name_tag_ui_position, set_kill_feed_style, set_name_tag_layout,
        write_respawn_status,
    };
    use crate::match_game::{LifeState, LifeStatus};
    use bevy::prelude::{BackgroundColor, Color, Node, UiTransform, Vec2, Visibility};

    #[test]
    fn living_players_have_no_spawn_protection_countdown_text() {
        let mut text = "stale shield text".to_owned();
        write_respawn_status(&mut text, LifeState::alive());
        assert!(text.is_empty());
    }

    #[test]
    fn dead_players_keep_the_respawn_countdown() {
        let mut text = String::new();
        write_respawn_status(
            &mut text,
            LifeState {
                status: LifeStatus::Respawning,
                respawn_remaining: 2.34,
            },
        );
        assert_eq!(text, "RESPAWN\n2.3");
    }

    #[test]
    fn projected_name_tag_coordinates_enter_scaled_ui_space() {
        let position = name_tag_ui_position(Vec2::new(450.0, 270.0), Vec2::ZERO, 1.5);
        assert!(position.abs_diff_eq(Vec2::new(300.0, 180.0), 1.0e-5));
    }

    #[test]
    fn projected_name_tag_coordinates_are_unchanged_at_reference_scale() {
        let position = name_tag_ui_position(Vec2::new(450.0, 270.0), Vec2::new(100.0, 30.0), 1.0);
        assert_eq!(position, Vec2::new(350.0, 240.0));
    }

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

    #[test]
    fn settled_kill_feed_does_not_rewrite_idle_style() {
        let mut transform = UiTransform::default();
        let mut background = BackgroundColor(Color::srgb(1.0, 0.0, 0.0));
        assert!(set_kill_feed_style(
            &mut transform,
            &mut background,
            1.0,
            false
        ));
        assert!(!set_kill_feed_style(
            &mut transform,
            &mut background,
            1.0,
            false
        ));
    }

    #[test]
    fn settled_name_tag_does_not_rewrite_idle_layout() {
        let mut node = Node::default();
        let mut visibility = Visibility::Hidden;
        let position = Vec2::new(120.0, 80.0);
        assert!(set_name_tag_layout(&mut node, &mut visibility, position));
        assert!(!set_name_tag_layout(&mut node, &mut visibility, position));
    }
}
