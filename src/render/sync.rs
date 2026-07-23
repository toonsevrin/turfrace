//! The single, deliberately small coupling point between simulation and presentation.

use bevy::prelude::*;

use crate::{
    board::BoardGrid,
    match_game::{
        Competitor, CompetitorKind, LifeState, MatchStatistics, Rankings, SpawnProtection,
    },
    movement::CompetitorMotion,
    territory_map::TerritoryMap,
    trail::ActiveTrail,
};

use super::trail::MAX_RENDER_TRAIL_POINTS;
use super::{CompetitorVisual, FieldVisual, TerritoryVisual, TrailVisual};

pub(super) fn sync_board_visuals(
    board: Option<ResMut<BoardGrid>>,
    vector_map: Option<Res<TerritoryMap>>,
    mut field: ResMut<FieldVisual>,
    mut territory: ResMut<TerritoryVisual>,
    mut last_generation_revision: Local<u64>,
    mut last_ownership_revision: Local<u64>,
    mut last_vector_revision: Local<u64>,
) {
    let Some(mut board) = board else { return };
    let vector_changed = vector_map
        .as_ref()
        .is_some_and(|map| map.revision != *last_vector_revision && !map.arena.is_empty());
    let generation_changed = *last_generation_revision != board.generation_revision;
    let mut ownership_changes = std::mem::take(&mut board.ownership_changes);
    let ownership_changed =
        *last_ownership_revision != board.ownership_revision || !ownership_changes.is_empty();
    if !generation_changed && !ownership_changed && !vector_changed {
        return;
    }
    if generation_changed {
        field.contour.clone_from(&board.contour.points);
        field.revision = field.revision.wrapping_add(1);
        *last_generation_revision = board.generation_revision;
    }
    let geometry_changed = generation_changed
        || territory.width != board.width
        || territory.height != board.height
        || territory.cell_size != board.cell_size
        || territory.origin != board.world_origin
        || territory.owners.len() != board.owner.len();
    if ownership_changed && !geometry_changed && ownership_changes.is_empty() {
        ownership_changes.extend(
            territory
                .owners
                .iter()
                .zip(&board.owner)
                .enumerate()
                .filter_map(|(index, (old, new))| {
                    (*old != new.0).then_some(crate::board::OwnershipChange {
                        index,
                        old: crate::ids::OwnerId(*old),
                        new: *new,
                    })
                }),
        );
    }
    if ownership_changed {
        for change in &ownership_changes {
            let old = territory
                .owners
                .get(change.index)
                .copied()
                .unwrap_or_default();
            if old != change.new.0
                && let Some(owner) = territory.owners.get_mut(change.index)
            {
                *owner = change.new.0;
            }
        }
    }
    if !geometry_changed && !ownership_changed && !vector_changed {
        return;
    }
    territory.width = board.width;
    territory.height = board.height;
    territory.cell_size = board.cell_size;
    territory.origin = board.world_origin;
    if geometry_changed {
        territory.owners.clear();
        territory
            .owners
            .extend(board.owner.iter().map(|owner| owner.0));
    }
    if vector_changed && let Some(map) = vector_map.as_ref() {
        territory.arena.clone_from(&map.arena);
        for index in 0..territory.polygons.len() {
            territory.polygons[index].clone_from(&map.territories[index]);
        }
        *last_vector_revision = map.revision;
    }
    territory.revision = territory.revision.wrapping_add(1);
    *last_ownership_revision = board.ownership_revision;
}

#[allow(clippy::type_complexity)]
pub(super) fn sync_competitor_snapshots(
    mut commands: Commands,
    rankings: Option<Res<Rankings>>,
    mut territory: ResMut<TerritoryVisual>,
    competitors: Query<(
        Entity,
        &Competitor,
        &CompetitorMotion,
        &LifeState,
        Option<&SpawnProtection>,
        Option<&MatchStatistics>,
        Option<&ActiveTrail>,
        Option<&CompetitorVisual>,
        Option<&TrailVisual>,
    )>,
) {
    let mut humans: Vec<_> = competitors
        .iter()
        .filter(|(_, competitor, ..)| competitor.kind == CompetitorKind::Human)
        .map(|(_, competitor, ..)| competitor.id)
        .collect();
    humans.sort();
    let leader = rankings.as_ref().and_then(|rankings| rankings.leader());
    for (entity, competitor, motion, life, protection, stats, trail, current, current_trail) in
        &competitors
    {
        let pattern_slot = competitor.id.index();
        if territory.pattern_ids[pattern_slot] != competitor.pattern_id
            || territory.color_ids[pattern_slot] != competitor.color_id
        {
            territory.pattern_ids[pattern_slot] = competitor.pattern_id;
            territory.color_ids[pattern_slot] = competitor.color_id;
            territory.revision = territory.revision.wrapping_add(1);
        }
        let human_slot = (competitor.kind == CompetitorKind::Human)
            .then(|| {
                humans
                    .binary_search(&competitor.id)
                    .ok()
                    .map(|slot| slot as u8)
            })
            .flatten();
        let next = CompetitorVisual {
            id: competitor.id.0,
            color_id: competitor.color_id,
            pattern_id: competitor.pattern_id,
            position: motion.position,
            previous_position: motion.previous_position,
            heading: motion.heading,
            human_slot,
            alive: life.is_alive(),
            spawn_protection: protection.map_or(0.0, |protection| protection.remaining),
            is_leader: leader == Some(competitor.id),
            awareness: 0.0,
            kills: stats.map_or(0, |stats| stats.kills),
            kill_streak: stats.map_or(0, |stats| stats.kill_streak),
        };
        let changed = current.is_none_or(|old| {
            old.position != next.position
                || old.heading != next.heading
                || old.alive != next.alive
                || old.spawn_protection != next.spawn_protection
                || old.is_leader != next.is_leader
                || old.kills != next.kills
                || old.kill_streak != next.kill_streak
                || old.human_slot != next.human_slot
                || old.color_id != next.color_id
        });
        if changed {
            commands.entity(entity).insert(next);
        }

        match trail {
            Some(trail) => {
                let trail_changed = trail_visual_needs_update(current_trail, trail);
                if trail_changed {
                    commands.entity(entity).insert(TrailVisual {
                        points: trail.render_points(MAX_RENDER_TRAIL_POINTS),
                        length: trail.length,
                        source_samples: trail.points.len(),
                        revision: current_trail.map_or(1, |old| old.revision.wrapping_add(1)),
                        dangerous: false,
                    });
                }
            }
            None if current_trail.is_some() => {
                commands.entity(entity).remove::<TrailVisual>();
            }
            None => {}
        }
    }
}

fn trail_visual_needs_update(current: Option<&TrailVisual>, trail: &ActiveTrail) -> bool {
    current.is_none_or(|visual| {
        visual.source_samples != trail.points.len()
            || visual
                .points
                .last()
                .is_none_or(|point| point.distance_squared(trail.head) > 1e-8)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{board::Cell, ids::CompetitorId, ids::OwnerId};

    fn tiny_board() -> BoardGrid {
        BoardGrid {
            width: 1,
            height: 1,
            cell_size: 0.5,
            field_mask: vec![true],
            owner: vec![OwnerId::UNCLAIMED],
            active_trail_bits: vec![0],
            contour: crate::board::FieldContour {
                points: vec![Vec2::X, Vec2::Y, Vec2::NEG_X],
                ..default()
            },
            generation_revision: 1,
            ownership_revision: 1,
            ..default()
        }
    }

    #[test]
    fn trail_bit_changes_do_not_rebuild_territory() {
        let mut app = App::new();
        app.init_resource::<FieldVisual>()
            .init_resource::<TerritoryVisual>()
            .insert_resource(tiny_board())
            .add_systems(Update, sync_board_visuals);
        app.update();
        let revision = app.world().resource::<TerritoryVisual>().revision;
        app.world_mut()
            .resource_mut::<BoardGrid>()
            .active_trail_bits[0] = 1;
        app.update();
        assert_eq!(app.world().resource::<TerritoryVisual>().revision, revision);

        {
            let mut board = app.world_mut().resource_mut::<BoardGrid>();
            board.owner[0] = OwnerId(1);
            board.ownership_revision += 1;
        }
        app.update();
        assert_eq!(
            app.world().resource::<TerritoryVisual>().revision,
            revision + 1
        );
    }

    #[test]
    fn trail_visual_tracks_the_exact_head_between_committed_samples() {
        let mut trail = ActiveTrail::new(CompetitorId(0), Cell::new(0, 0), Vec2::ZERO, Vec2::X);
        let visual = TrailVisual {
            points: trail.render_points(MAX_RENDER_TRAIL_POINTS),
            source_samples: trail.points.len(),
            ..default()
        };

        trail.head = Vec2::new(0.1, 0.0);
        assert!(trail_visual_needs_update(Some(&visual), &trail));
        let visual = TrailVisual {
            points: trail.render_points(MAX_RENDER_TRAIL_POINTS),
            source_samples: trail.points.len(),
            ..default()
        };
        assert!(!trail_visual_needs_update(Some(&visual), &trail));
        trail.points.push(trail.head);
        assert!(trail_visual_needs_update(Some(&visual), &trail));
    }
}
