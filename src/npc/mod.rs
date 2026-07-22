use bevy::prelude::*;

use crate::{board::BoardGrid, ids::CompetitorId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NpcDifficulty {
    Easy,
    Normal,
    Hard,
}
impl NpcDifficulty {
    pub fn think_hz(self) -> f32 {
        match self {
            Self::Easy => 5.0,
            Self::Normal => 8.0,
            Self::Hard => 12.0,
        }
    }
    pub fn perception(self) -> f32 {
        match self {
            Self::Easy => 16.0,
            Self::Normal => 22.0,
            Self::Hard => 28.0,
        }
    }
    pub fn error_radians(self) -> f32 {
        match self {
            Self::Easy => 12.0_f32.to_radians(),
            Self::Normal => 6.0_f32.to_radians(),
            Self::Hard => 2.0_f32.to_radians(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NpcPersonality {
    Cautious,
    Balanced,
    Raider,
    Greedy,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NpcDebugState {
    Recovering,
    Patrolling,
    Expanding,
    Returning,
    Hunting,
    Evading,
    EdgeAvoidance,
}

#[derive(Clone, Copy, Debug)]
pub struct NpcSelfState {
    pub id: CompetitorId,
    pub position: Vec2,
    pub heading: Vec2,
    pub protected: bool,
    pub trail_length: f32,
    pub owns_current_cell: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct PerceivedCompetitor {
    pub id: CompetitorId,
    pub position: Vec2,
    pub alive: bool,
    pub territory_cells: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct PerceivedTrail {
    pub owner: CompetitorId,
    pub nearest_point: Vec2,
    pub distance: f32,
}
#[derive(Clone, Debug, Default)]
pub struct RankingSnapshot {
    pub ordered: Vec<CompetitorId>,
}

pub struct BoardQuery<'a> {
    board: &'a BoardGrid,
    player: CompetitorId,
}
impl<'a> BoardQuery<'a> {
    pub fn new(board: &'a BoardGrid, player: CompetitorId) -> Self {
        Self { board, player }
    }
    pub fn signed_distance(&self, p: Vec2) -> f32 {
        self.board.signed_distance_at(p)
    }
    pub fn owns(&self, p: Vec2) -> bool {
        self.board
            .world_to_cell(p)
            .is_some_and(|c| self.board.owns(c, self.player))
    }
    pub fn nearest_owned(&self, p: Vec2) -> Option<Vec2> {
        self.board.nearest_owned_cell_center(p, self.player)
    }
    pub fn inward_normal(&self, p: Vec2) -> Vec2 {
        self.board.inward_normal(p)
    }
    pub fn playable_cells(&self) -> u32 {
        self.board.playable_cells
    }
}

pub struct NpcContext<'a> {
    pub self_state: NpcSelfState,
    pub nearby_competitors: &'a [PerceivedCompetitor],
    pub nearby_trails: &'a [PerceivedTrail],
    pub board: &'a BoardQuery<'a>,
    pub ranking: &'a RankingSnapshot,
    pub match_time: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct NpcIntent {
    pub desired_direction: Vec2,
    pub debug_state: NpcDebugState,
}

pub trait NpcBrain: Send + Sync + 'static {
    fn tick(&mut self, context: &NpcContext, delta_seconds: f32) -> NpcIntent;
    fn on_event(&mut self, _event: &NpcEvent) {}
}
#[derive(Clone, Copy, Debug)]
pub enum NpcEvent {
    Spawned,
    Died,
    TerritoryStolen,
}

#[derive(Component)]
pub struct NpcController {
    pub brain: Box<dyn NpcBrain>,
    pub think_remaining: f32,
    pub last_intent: NpcIntent,
    pub difficulty: NpcDifficulty,
    pub personality: NpcPersonality,
}
impl NpcController {
    pub fn standard(
        id: CompetitorId,
        personality: NpcPersonality,
        difficulty: NpcDifficulty,
    ) -> Self {
        Self {
            brain: Box::new(DefaultNpcBrain::new(id, personality)),
            think_remaining: 0.0,
            last_intent: NpcIntent {
                desired_direction: Vec2::Y,
                debug_state: NpcDebugState::Patrolling,
            },
            difficulty,
            personality,
        }
    }
}

pub struct DefaultNpcBrain {
    id: CompetitorId,
    personality: NpcPersonality,
    phase: f32,
    outside_target: f32,
    side: f32,
}
impl DefaultNpcBrain {
    pub fn new(id: CompetitorId, personality: NpcPersonality) -> Self {
        let phase = (id.0 as f32 * 2.399_963_1).rem_euclid(std::f32::consts::TAU);
        let outside_target = match personality {
            NpcPersonality::Cautious => 4.0,
            NpcPersonality::Balanced => 7.0,
            NpcPersonality::Raider => 5.0,
            NpcPersonality::Greedy => 11.0,
        };
        Self {
            id,
            personality,
            phase,
            outside_target,
            side: if id.0.is_multiple_of(2) { 1.0 } else { -1.0 },
        }
    }
}
impl NpcBrain for DefaultNpcBrain {
    fn tick(&mut self, c: &NpcContext, dt: f32) -> NpcIntent {
        self.phase = (self.phase + dt * 0.3).rem_euclid(std::f32::consts::TAU);
        let s = c.self_state;
        if s.protected {
            return NpcIntent {
                desired_direction: (s.heading + perpendicular(s.heading) * self.side * 0.22)
                    .normalize_or(s.heading),
                debug_state: NpcDebugState::Recovering,
            };
        }
        if c.board.signed_distance(s.position) < 2.0 {
            return NpcIntent {
                desired_direction: (c.board.inward_normal(s.position)
                    + perpendicular(s.heading) * self.side * 0.35)
                    .normalize_or_zero(),
                debug_state: NpcDebugState::EdgeAvoidance,
            };
        }
        if s.trail_length > 0.0 {
            if self.personality == NpcPersonality::Raider
                && let Some(target) = c
                    .nearby_trails
                    .iter()
                    .filter(|t| t.owner != self.id)
                    .min_by(|a, b| a.distance.total_cmp(&b.distance))
                && target.distance < s.trail_length.min(5.0)
            {
                return NpcIntent {
                    desired_direction: (target.nearest_point - s.position).normalize_or_zero(),
                    debug_state: NpcDebugState::Hunting,
                };
            }
            let threatened = c
                .nearby_competitors
                .iter()
                .filter(|p| p.alive && p.id != self.id)
                .any(|p| p.position.distance(s.position) < 5.0);
            if (s.trail_length >= self.outside_target || threatened)
                && let Some(home) = c.board.nearest_owned(s.position)
            {
                return NpcIntent {
                    desired_direction: (home - s.position).normalize_or_zero(),
                    debug_state: if threatened {
                        NpcDebugState::Evading
                    } else {
                        NpcDebugState::Returning
                    },
                };
            }
            let curve = perpendicular(s.heading) * self.side * 0.42;
            return NpcIntent {
                desired_direction: (s.heading + curve).normalize_or_zero(),
                debug_state: NpcDebugState::Expanding,
            };
        }
        if self.personality != NpcPersonality::Cautious
            && let Some(target) = c
                .nearby_trails
                .iter()
                .min_by(|a, b| a.distance.total_cmp(&b.distance))
            && target.distance
                < if self.personality == NpcPersonality::Raider {
                    7.0
                } else {
                    4.0
                }
        {
            return NpcIntent {
                desired_direction: (target.nearest_point - s.position).normalize_or(s.heading),
                debug_state: NpcDebugState::Hunting,
            };
        }

        NpcIntent {
            desired_direction: patrol_direction(s, c.nearby_competitors, self.side, self.phase),
            debug_state: NpcDebugState::Patrolling,
        }
    }
}

fn patrol_direction(
    state: NpcSelfState,
    nearby: &[PerceivedCompetitor],
    side: f32,
    phase: f32,
) -> Vec2 {
    let outward = state.position.normalize_or(Vec2::from_angle(phase));
    let mut desired = outward + perpendicular(outward) * side * 0.24;
    if let Some(enemy) = nearby.iter().filter(|enemy| enemy.alive).min_by(|a, b| {
        a.position
            .distance_squared(state.position)
            .total_cmp(&b.position.distance_squared(state.position))
    }) {
        let distance = enemy.position.distance(state.position);
        if distance < 4.5 {
            desired += (state.position - enemy.position).normalize_or_zero()
                * (1.0 - distance / 4.5)
                * 1.8;
        }
    }
    desired.normalize_or(state.heading)
}

fn perpendicular(v: Vec2) -> Vec2 {
    Vec2::new(-v.y, v.x)
}

pub fn deterministic_npc_name(index: usize) -> &'static str {
    const NAMES: [&str; 12] = [
        "BIX", "MOKO", "ZAP", "TILLY", "CRUMB", "NOVA", "PIP", "RUNE", "JUNO", "KIP", "DOT",
        "WOBBLE",
    ];
    NAMES[index % NAMES.len()]
}
pub fn deterministic_personality(seed: u64, id: CompetitorId) -> NpcPersonality {
    match ((seed.rotate_left(id.0 as u32) ^ id.0 as u64) % 10) as u8 {
        0..=2 => NpcPersonality::Cautious,
        3..=7 => NpcPersonality::Balanced,
        8..=9 => NpcPersonality::Raider,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn difficulty_never_changes_speed() {
        assert!(NpcDifficulty::Hard.think_hz() > NpcDifficulty::Easy.think_hz());
        assert!(NpcDifficulty::Hard.error_radians() < NpcDifficulty::Easy.error_radians());
    }
    #[test]
    fn names_and_personalities_are_deterministic() {
        assert_eq!(deterministic_npc_name(2), deterministic_npc_name(14));
        assert_eq!(
            deterministic_personality(42, CompetitorId(3)),
            deterministic_personality(42, CompetitorId(3))
        );
    }

    #[test]
    fn patrol_steering_avoids_a_close_competitor() {
        let state = NpcSelfState {
            id: CompetitorId(0),
            position: Vec2::new(8.0, 0.0),
            heading: Vec2::X,
            protected: false,
            trail_length: 0.0,
            owns_current_cell: true,
        };
        let enemy = PerceivedCompetitor {
            id: CompetitorId(1),
            position: Vec2::new(8.5, 0.0),
            alive: true,
            territory_cells: 1,
        };
        let unopposed = patrol_direction(state, &[], 1.0, 0.0);
        let avoiding = patrol_direction(state, &[enemy], 1.0, 0.0);
        assert!(avoiding.x < unopposed.x);
    }
}
