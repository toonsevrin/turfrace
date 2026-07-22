//! The single, deliberately small coupling point between simulation and presentation.

use bevy::prelude::*;

use crate::{
    board::BoardGrid,
    match_game::{Competitor, CompetitorKind, LifeState, Rankings, SpawnProtection},
    movement::CompetitorMotion,
    trail::ActiveTrail,
};

use super::{CompetitorVisual, FieldVisual, TerritoryVisual, TrailVisual};

pub(super) fn sync_board_visuals(
    board: Option<Res<BoardGrid>>,
    mut field: ResMut<FieldVisual>,
    mut territory: ResMut<TerritoryVisual>,
    mut last_generation_revision: Local<u64>,
    mut last_ownership_revision: Local<u64>,
) {
    let Some(board) = board else { return };
    let generation_changed = *last_generation_revision != board.generation_revision;
    let ownership_changed = *last_ownership_revision != board.ownership_revision;
    if !generation_changed && !ownership_changed {
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
    let mut dirty = std::collections::HashSet::new();
    let mut promoted_owners = Vec::new();
    let mut promoted = [false; 13];
    let mut promote = |owner: u8| {
        if (1..=12).contains(&owner) && !promoted[owner as usize] {
            promoted[owner as usize] = true;
            promoted_owners.push(owner);
        }
    };
    if geometry_changed {
        territory.layer_order = [0; 12];
        for owner in &board.owner {
            promote(owner.0);
        }
    }
    if ownership_changed {
        for (index, (old, new)) in territory.owners.iter().zip(&board.owner).enumerate() {
            if *old == new.0 {
                continue;
            }
            promote(new.0);
            if geometry_changed {
                continue;
            }
            let x = index as u32 % board.width;
            let y = index as u32 / board.width;
            let chunk = UVec2::new(
                x / super::territory::CHUNK_SIZE,
                y / super::territory::CHUNK_SIZE,
            );
            dirty.insert(chunk);
            if x.is_multiple_of(super::territory::CHUNK_SIZE) && chunk.x > 0 {
                dirty.insert(chunk - UVec2::X);
            }
            if y.is_multiple_of(super::territory::CHUNK_SIZE) && chunk.y > 0 {
                dirty.insert(chunk - UVec2::Y);
            }
            if x % super::territory::CHUNK_SIZE == super::territory::CHUNK_SIZE - 1 {
                dirty.insert(chunk + UVec2::X);
            }
            if y % super::territory::CHUNK_SIZE == super::territory::CHUNK_SIZE - 1 {
                dirty.insert(chunk + UVec2::Y);
            }
        }
    }
    if !geometry_changed && (!ownership_changed || dirty.is_empty()) {
        return;
    }
    territory.width = board.width;
    territory.height = board.height;
    territory.cell_size = board.cell_size;
    territory.origin = board.world_origin;
    territory.owners.clear();
    territory
        .owners
        .extend(board.owner.iter().map(|owner| owner.0));
    territory.playable.clone_from(&board.field_mask);
    territory.promote_layers(promoted_owners);
    territory.rebuild_contours();
    territory.revision = territory.revision.wrapping_add(1);
    territory.dirty_chunks = if geometry_changed {
        Vec::new()
    } else {
        dirty.into_iter().collect()
    };
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
    for (entity, competitor, motion, life, protection, trail, current, current_trail) in
        &competitors
    {
        let pattern_slot = competitor.id.index();
        if territory.pattern_ids[pattern_slot] != competitor.pattern_id
            || territory.color_ids[pattern_slot] != competitor.color_id
        {
            territory.pattern_ids[pattern_slot] = competitor.pattern_id;
            territory.color_ids[pattern_slot] = competitor.color_id;
            territory.revision = territory.revision.wrapping_add(1);
            territory.dirty_chunks.clear();
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
        };
        let changed = current.is_none_or(|old| {
            old.position != next.position
                || old.heading != next.heading
                || old.alive != next.alive
                || old.spawn_protection != next.spawn_protection
                || old.is_leader != next.is_leader
                || old.human_slot != next.human_slot
                || old.color_id != next.color_id
        });
        if changed {
            commands.entity(entity).insert(next);
        }

        match trail {
            Some(trail) => {
                let trail_changed = current_trail.is_none_or(|old| {
                    old.points.len() != trail.points.len()
                        || old.points.last() != trail.points.last()
                });
                if trail_changed {
                    commands.entity(entity).insert(TrailVisual {
                        points: trail.points.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::OwnerId;

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
}
