use bevy::prelude::*;

use crate::{board::BoardGrid, ids::CompetitorId, territory_map::TerritoryMap};

use super::{
    CapturePlan, NpcDecisionFrame, NpcMatchMemory, NpcTraits, NpcVisibleRival, NpcVisibleTrail,
};

#[allow(clippy::too_many_arguments)]
pub fn build_decision_frame(
    board: &BoardGrid,
    territory: &TerritoryMap,
    self_id: CompetitorId,
    position: Vec2,
    heading: Vec2,
    protected: bool,
    trail_length: f32,
    visible_rank: u8,
    rivals: &[NpcVisibleRival],
    trails: &[NpcVisibleTrail],
    traits: NpcTraits,
    memory: &NpcMatchMemory,
    proposed_capture: Option<CapturePlan>,
) -> NpcDecisionFrame {
    let radius = traits.perception_radius();
    let heading = heading.normalize_or(Vec2::Y);
    let mut nearest_rivals = [None; 3];
    for (slot, rival) in rivals
        .iter()
        .filter(|rival| rival.distance <= radius)
        .take(nearest_rivals.len())
        .enumerate()
    {
        nearest_rivals[slot] = Some(*rival);
    }
    let mut nearest_trails = [None; 2];
    for (slot, trail) in trails
        .iter()
        .filter(|trail| trail.distance <= radius)
        .take(nearest_trails.len())
        .enumerate()
    {
        nearest_trails[slot] = Some(*trail);
    }

    NpcDecisionFrame {
        position,
        heading,
        protected,
        owns_current_cell: territory.owns(position, self_id),
        trail_length,
        visible_rank,
        edge_distance: territory.arena_signed_distance(position),
        inward_direction: territory.arena_inward_normal(position),
        home: memory
            .capture_home()
            .or_else(|| board.nearest_owner_frontier(position, self_id, radius * 1.6)),
        active_waypoint: memory.active_waypoint(),
        proposed_capture,
        quiet_direction: quiet_direction(board, self_id, position, heading, &nearest_rivals),
        rivals: nearest_rivals,
        trails: nearest_trails,
    }
}

fn quiet_direction(
    board: &BoardGrid,
    self_id: CompetitorId,
    position: Vec2,
    heading: Vec2,
    rivals: &[Option<NpcVisibleRival>; 3],
) -> Vec2 {
    (0..8)
        .map(|index| {
            let direction = rotate(heading, std::f32::consts::TAU * index as f32 / 8.0);
            let point = position + direction * 8.0;
            let ownership = board.world_to_cell(point).map_or(-2.0, |cell| {
                let owner = board.owner_at(cell);
                if owner == self_id.owner() {
                    1.0
                } else if owner == crate::ids::OwnerId::UNCLAIMED {
                    0.0
                } else {
                    -1.0
                }
            });
            let clearance = rivals
                .iter()
                .flatten()
                .map(|rival| point.distance(position + rival.relative))
                .fold(12.0_f32, f32::min)
                / 12.0;
            (direction, -ownership + clearance)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(heading, |choice| choice.0)
}

fn rotate(direction: Vec2, angle: f32) -> Vec2 {
    let (sin, cos) = angle.sin_cos();
    Vec2::new(
        direction.x * cos - direction.y * sin,
        direction.x * sin + direction.y * cos,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensing_excludes_rivals_and_trails_outside_the_skill_radius() {
        let config = crate::config::GameConfig::default();
        let board = BoardGrid::generate(9, 2, &config);
        let territory = TerritoryMap::from_board(&board);
        let traits = NpcTraits {
            skill: 0.0,
            aggression: 0.5,
            greed: 0.5,
            exploration: 0.5,
            composure: 0.5,
            adaptability: 0.5,
            commitment: 0.5,
            turning_bias: 0.0,
        };
        let rival = NpcVisibleRival {
            id: CompetitorId(1),
            relative: Vec2::X * 14.01,
            distance: 14.01,
            territory_share: 0.2,
            exposed: true,
        };
        let trail = NpcVisibleTrail {
            owner: CompetitorId(1),
            relative: Vec2::X * 14.01,
            distance: 14.01,
            own: false,
        };
        let frame = build_decision_frame(
            &board,
            &territory,
            CompetitorId(0),
            Vec2::ZERO,
            Vec2::Y,
            false,
            0.0,
            1,
            &[rival],
            &[trail],
            traits,
            &NpcMatchMemory::new(true),
            None,
        );
        assert_eq!(frame.rivals, [None; 3]);
        assert_eq!(frame.trails, [None; 2]);
    }
}
