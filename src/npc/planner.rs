use bevy::prelude::*;

use crate::{
    board::{BoardGrid, OwnerFrontier},
    ids::CompetitorId,
    territory_map::TerritoryMap,
};

use super::{CapturePlan, NpcMatchMemory, NpcTraits, NpcVisibleRival};

const FRONTIER_SAMPLE_CAP: usize = 72;

pub struct CapturePlanContext<'a> {
    pub board: &'a BoardGrid,
    pub territory: &'a TerritoryMap,
    pub id: CompetitorId,
    pub position: Vec2,
    pub heading: Vec2,
    pub visible_rank: u8,
    pub traits: NpcTraits,
    pub memory: &'a NpcMatchMemory,
    pub rivals: &'a [NpcVisibleRival],
}

pub fn propose_capture_plan(
    context: CapturePlanContext<'_>,
    frontier_scratch: &mut Vec<OwnerFrontier>,
) -> Option<CapturePlan> {
    let CapturePlanContext {
        board,
        territory,
        id,
        position,
        heading,
        visible_rank,
        traits,
        memory,
        rivals,
    } = context;
    board.collect_owner_frontiers(position, id, traits.perception_radius(), frontier_scratch);
    if frontier_scratch.len() < 2 {
        return None;
    }

    let risk_budget = capture_risk_budget(visible_rank, traits, memory, rivals.iter());
    let desired_depth = 7.0 + risk_budget * 15.0;
    let desired_width = 6.0 + (traits.greed * 0.55 + risk_budget * 0.45) * 14.0;
    let desired_area = desired_depth * desired_width;
    let sample_step = frontier_scratch.len().div_ceil(FRONTIER_SAMPLE_CAP).max(1);
    let preferred_side = if traits.turning_bias >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let mut best: Option<(CapturePlan, f32)> = None;

    for exit in frontier_scratch.iter().step_by(sample_step) {
        let Some(staging_exit) = first_point_with_ownership(
            territory,
            id,
            exit.position,
            -exit.outward,
            true,
            board.cell_size,
        ) else {
            continue;
        };
        if staging_exit.distance(position) < board.cell_size.max(1.5)
            || !segment_has_ownership(territory, id, position, staging_exit, true)
        {
            continue;
        }
        let Some(boundary_exit) = first_point_with_ownership(
            territory,
            id,
            exit.position,
            exit.outward,
            false,
            board.cell_size,
        ) else {
            continue;
        };
        let lateral = exit.outward.perp();
        for home in frontier_scratch.iter().step_by(sample_step) {
            let separation = home.position - exit.position;
            let width = separation.length();
            if width < desired_width * 0.30 || width > desired_width * 1.65 {
                continue;
            }
            let signed_side = separation.dot(lateral);
            if signed_side.abs() < width * 0.45 {
                continue;
            }
            let Some(inside_home) = first_point_with_ownership(
                territory,
                id,
                home.position,
                -home.outward,
                true,
                board.cell_size,
            ) else {
                continue;
            };

            let side = signed_side.signum();
            let outbound_apex = boundary_exit + exit.outward * desired_depth;
            let return_apex = outbound_apex + lateral * side * width;
            if territory.owns(outbound_apex, id)
                || territory.owns(return_apex, id)
                || territory.arena_signed_distance(outbound_apex) < 2.5
                || territory.arena_signed_distance(return_apex) < 2.5
            {
                continue;
            }
            let estimated_area = desired_depth * width;
            if estimated_area < 8.0 {
                continue;
            }

            let route_clearance = rivals
                .iter()
                .map(|rival| {
                    let rival_position = position + rival.relative;
                    rival_position
                        .distance(outbound_apex)
                        .min(rival_position.distance(return_apex))
                        .min(rival_position.distance(staging_exit))
                        .min(rival_position.distance(inside_home))
                })
                .fold(traits.perception_radius(), f32::min)
                / traits.perception_radius();
            let heading_alignment = heading
                .normalize_or(Vec2::Y)
                .dot((staging_exit - position).normalize_or(heading));
            let area_fit = 1.0 - ((estimated_area - desired_area).abs() / desired_area.max(1.0));
            let side_preference = if side == preferred_side { 0.12 } else { 0.0 };
            let score = area_fit * 1.6
                + route_clearance * (0.45 + traits.composure * 0.35)
                + heading_alignment * 0.18
                + side_preference;
            let plan = CapturePlan {
                // Turn and line up while still safe. Pointing directly at an
                // exterior waypoint from an arbitrary interior heading makes
                // finite-turn motion skim out and back through the same edge,
                // producing the tiny accidental loops this planner replaces.
                waypoints: [staging_exit, outbound_apex, return_apex, inside_home],
                estimated_area,
                risk_budget,
            };
            if best.is_none_or(|(_, current)| score > current) {
                best = Some((plan, score));
            }
        }
    }
    best.map(|(plan, _)| plan)
}

fn segment_has_ownership(
    territory: &TerritoryMap,
    id: CompetitorId,
    start: Vec2,
    end: Vec2,
    owned: bool,
) -> bool {
    (0..=12).all(|step| {
        let point = start.lerp(end, step as f32 / 12.0);
        territory.owns(point, id) == owned
    })
}

pub fn capture_risk_budget<'a>(
    visible_rank: u8,
    traits: NpcTraits,
    memory: &NpcMatchMemory,
    rivals: impl IntoIterator<Item = &'a NpcVisibleRival>,
) -> f32 {
    let rank_pressure = f32::from(visible_rank.saturating_sub(1)).min(11.0) / 11.0;
    let nearest_pressure = rivals
        .into_iter()
        .map(|rival| {
            let learned_aggression = memory.opponents[rival.id.index()].aggression;
            (1.0 - rival.distance / traits.perception_radius()).clamp(0.0, 1.0)
                * (0.65 + learned_aggression * 0.35)
        })
        .fold(0.0_f32, f32::max);
    let caution = nearest_pressure * (0.34 - traits.composure * 0.22);
    (0.18
        + traits.greed * 0.30
        + traits.exploration * 0.12
        + traits.aggression * 0.08
        + memory.confidence * 0.10
        + memory.frustration * 0.10
        + rank_pressure * 0.16
        - caution)
        .clamp(0.12, 0.95)
}

fn first_point_with_ownership(
    territory: &TerritoryMap,
    id: CompetitorId,
    origin: Vec2,
    direction: Vec2,
    owned: bool,
    cell_size: f32,
) -> Option<Vec2> {
    let direction = direction.normalize_or(Vec2::Y);
    (0..=8)
        .map(|step| origin + direction * cell_size * (0.5 + step as f32 * 0.5))
        .find(|point| territory.owns(*point, id) == owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::GameConfig, territory_map::TerritoryMap};

    #[test]
    fn plan_leaves_owned_ground_and_returns_through_a_distinct_frontier() {
        let (board, territory) = territory_disk(10.0);
        let mut scratch = Vec::new();
        let plan = propose_capture_plan(
            CapturePlanContext {
                board: &board,
                territory: &territory,
                id: CompetitorId(0),
                position: Vec2::ZERO,
                heading: Vec2::X,
                visible_rank: 4,
                traits: traits(0.6),
                memory: &NpcMatchMemory::new(true),
                rivals: &[],
            },
            &mut scratch,
        )
        .expect("a visible frontier supports a capture plan");
        assert!(territory.owns(plan.waypoints[0], CompetitorId(0)));
        assert!(!territory.owns(plan.waypoints[1], CompetitorId(0)));
        assert!(!territory.owns(plan.waypoints[2], CompetitorId(0)));
        assert!(territory.owns(plan.waypoints[3], CompetitorId(0)));
        assert!(plan.waypoints[0].distance(plan.waypoints[3]) > 4.0);
    }

    #[test]
    fn plan_exists_for_the_coarse_starting_territory_used_by_the_soak() {
        let config = GameConfig {
            cell_size: 2.0,
            ..default()
        };
        let mut board = BoardGrid::generate(91, 4, &config);
        let mut territory = TerritoryMap::from_board(&board);
        territory.seed_owner(Vec2::ZERO, 4.0, CompetitorId(0));
        territory.rebuild_sample_cache(&mut board);
        let mut scratch = Vec::new();
        let plan = propose_capture_plan(
            CapturePlanContext {
                board: &board,
                territory: &territory,
                id: CompetitorId(0),
                position: Vec2::ZERO,
                heading: Vec2::X,
                visible_rank: 4,
                traits: traits(0.6),
                memory: &NpcMatchMemory::new(true),
                rivals: &[],
            },
            &mut scratch,
        );
        assert!(plan.is_some(), "frontiers={}", scratch.len());
    }

    #[test]
    fn greed_and_trailing_rank_produce_materially_larger_capture_plans() {
        let (board, territory) = territory_disk(10.0);
        let mut scratch = Vec::new();
        let cautious = propose_capture_plan(
            CapturePlanContext {
                board: &board,
                territory: &territory,
                id: CompetitorId(0),
                position: Vec2::ZERO,
                heading: Vec2::X,
                visible_rank: 1,
                traits: traits(0.05),
                memory: &NpcMatchMemory::new(true),
                rivals: &[],
            },
            &mut scratch,
        )
        .unwrap();
        let greedy = propose_capture_plan(
            CapturePlanContext {
                board: &board,
                territory: &territory,
                id: CompetitorId(0),
                position: Vec2::ZERO,
                heading: Vec2::X,
                visible_rank: 10,
                traits: traits(0.95),
                memory: &NpcMatchMemory::new(true),
                rivals: &[],
            },
            &mut scratch,
        )
        .unwrap();
        assert!(greedy.estimated_area > cautious.estimated_area * 1.35);
        assert!(greedy.risk_budget > cautious.risk_budget);
    }

    #[test]
    fn learned_nearby_aggression_reduces_risk_for_composed_planning() {
        let mut memory = NpcMatchMemory::new(true);
        memory.opponents[1].aggression = 1.0;
        let rival = NpcVisibleRival {
            id: CompetitorId(1),
            distance: 2.0,
            ..default()
        };
        let exposed = capture_risk_budget(5, traits(0.6), &memory, [].iter());
        let threatened = capture_risk_budget(5, traits(0.6), &memory, [&rival]);
        assert!(threatened < exposed);
    }

    fn territory_disk(radius: f32) -> (BoardGrid, TerritoryMap) {
        let mut board = BoardGrid::generate(91, 2, &GameConfig::default());
        let mut territory = TerritoryMap::from_board(&board);
        territory.seed_owner(Vec2::ZERO, radius, CompetitorId(0));
        territory.rebuild_sample_cache(&mut board);
        (board, territory)
    }

    fn traits(greed: f32) -> NpcTraits {
        NpcTraits {
            skill: 0.8,
            aggression: 0.5,
            greed,
            exploration: 0.5,
            composure: 0.6,
            adaptability: 0.7,
            commitment: 0.5,
            turning_bias: 0.2,
        }
    }
}
