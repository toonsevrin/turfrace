use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ids::{CompetitorId, MAX_COMPETITORS};

pub const NPC_WAYPOINT_CAPACITY: usize = 4;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum NpcDifficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl NpcDifficulty {
    pub const ALL: [Self; 3] = [Self::Easy, Self::Normal, Self::Hard];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Easy => "EASY",
            Self::Normal => "NORMAL",
            Self::Hard => "HARD",
        }
    }

    pub fn cycle(self, delta: i8) -> Self {
        Self::ALL[(self as i8 + delta).rem_euclid(Self::ALL.len() as i8) as usize]
    }

    pub const fn skill_bounds(self) -> (f32, f32, f32) {
        match self {
            Self::Easy => (0.05, 0.60, 0.25),
            Self::Normal => (0.25, 0.85, 0.55),
            Self::Hard => (0.45, 0.95, 0.75),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NpcBrainKind {
    Tactical,
    LegacyWanderer,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcTraits {
    pub skill: f32,
    pub aggression: f32,
    pub greed: f32,
    pub exploration: f32,
    pub composure: f32,
    pub adaptability: f32,
    pub commitment: f32,
    /// -1 is a strong right preference and +1 a strong left preference.
    pub turning_bias: f32,
}

impl NpcTraits {
    pub fn think_hz(self) -> f32 {
        4.0 + 8.0 * self.skill.clamp(0.0, 1.0)
    }

    pub fn perception_radius(self) -> f32 {
        14.0 + 14.0 * self.skill.clamp(0.0, 1.0)
    }

    pub fn minimum_commitment(self) -> f32 {
        0.10 + (1.0 - self.skill.clamp(0.0, 1.0)) * 0.30 + self.commitment.clamp(0.0, 1.0) * 0.25
    }

    pub fn error_radians(self) -> f32 {
        (14.0 + (1.0 - 14.0) * self.skill.clamp(0.0, 1.0)).to_radians()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum NpcAction {
    #[default]
    Continue,
    BeginCapture,
    FollowCapturePlan,
    ReturnHome,
    HuntTrail,
    StealLeader,
    Explore,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CapturePlan {
    pub waypoints: [Vec2; NPC_WAYPOINT_CAPACITY],
    pub estimated_area: f32,
    pub risk_budget: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NpcVisibleRival {
    pub id: CompetitorId,
    pub relative: Vec2,
    pub distance: f32,
    pub territory_share: f32,
    pub exposed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NpcVisibleTrail {
    pub owner: CompetitorId,
    pub relative: Vec2,
    pub distance: f32,
    pub own: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct NpcDecisionFrame {
    pub position: Vec2,
    pub heading: Vec2,
    pub protected: bool,
    pub owns_current_cell: bool,
    pub trail_length: f32,
    pub visible_rank: u8,
    pub edge_distance: f32,
    pub inward_direction: Vec2,
    pub home: Option<Vec2>,
    pub active_waypoint: Option<Vec2>,
    pub proposed_capture: Option<CapturePlan>,
    pub quiet_direction: Vec2,
    pub rivals: [Option<NpcVisibleRival>; 3],
    pub trails: [Option<NpcVisibleTrail>; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcDecision {
    pub action: NpcAction,
    pub desired_direction: Vec2,
    pub risk_budget: f32,
    pub commitment_seconds: f32,
    pub capture_plan: Option<CapturePlan>,
}

impl NpcDecision {
    pub fn continue_in(direction: Vec2) -> Self {
        Self {
            action: NpcAction::Continue,
            desired_direction: direction,
            risk_budget: 0.5,
            commitment_seconds: 0.2,
            capture_plan: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OpponentEstimate {
    pub aggression: f32,
    pub observations: u16,
}

#[derive(Clone, Debug)]
pub struct NpcMatchMemory {
    pub current_action: NpcAction,
    pub waypoints: [Vec2; NPC_WAYPOINT_CAPACITY],
    pub waypoint_count: u8,
    pub waypoint_index: u8,
    pub commitment_remaining: f32,
    pub confidence: f32,
    pub frustration: f32,
    pub active_plan_risk: f32,
    pub planned_capture_area: f32,
    pub opponents: [OpponentEstimate; MAX_COMPETITORS],
    pub adapts: bool,
    pub decision_counter: u64,
}

impl NpcMatchMemory {
    pub fn new(adapts: bool) -> Self {
        Self {
            current_action: NpcAction::Continue,
            waypoints: [Vec2::ZERO; NPC_WAYPOINT_CAPACITY],
            waypoint_count: 0,
            waypoint_index: 0,
            commitment_remaining: 0.0,
            confidence: 0.5,
            frustration: 0.0,
            active_plan_risk: 0.5,
            planned_capture_area: 0.0,
            opponents: [OpponentEstimate::default(); MAX_COMPETITORS],
            adapts,
            decision_counter: 0,
        }
    }

    pub fn advance(&mut self, delta_seconds: f32) {
        self.commitment_remaining = (self.commitment_remaining - delta_seconds).max(0.0);
        self.confidence += (0.5 - self.confidence) * (delta_seconds * 0.08).min(1.0);
        self.frustration *= (-delta_seconds * 0.12).exp();
    }

    pub fn active_waypoint(&self) -> Option<Vec2> {
        (self.waypoint_index < self.waypoint_count)
            .then(|| self.waypoints[self.waypoint_index as usize])
    }

    pub fn capture_home(&self) -> Option<Vec2> {
        (self.waypoint_count > 0).then(|| self.waypoints[self.waypoint_count as usize - 1])
    }

    pub fn begin_capture(&mut self, plan: CapturePlan) {
        self.waypoints = plan.waypoints;
        self.waypoint_count = NPC_WAYPOINT_CAPACITY as u8;
        self.waypoint_index = 0;
        self.active_plan_risk = plan.risk_budget;
        self.planned_capture_area = plan.estimated_area;
    }

    pub fn advance_waypoint_if_reached(&mut self, position: Vec2, threshold: f32) {
        if self
            .active_waypoint()
            .is_some_and(|waypoint| waypoint.distance(position) < threshold)
        {
            self.waypoint_index += 1;
            if self.waypoint_index >= self.waypoint_count {
                self.clear_capture_plan();
            }
        }
    }

    pub fn clear_capture_plan(&mut self) {
        self.waypoint_count = 0;
        self.waypoint_index = 0;
        self.planned_capture_area = 0.0;
    }

    pub fn on_event(&mut self, event: NpcEvent, adaptability: f32) {
        match event {
            NpcEvent::Spawned => {
                self.commitment_remaining = 0.0;
                self.current_action = NpcAction::Continue;
                self.clear_capture_plan();
            }
            NpcEvent::Died { killer } => {
                self.confidence = (self.confidence - 0.22).max(0.0);
                self.frustration = (self.frustration + 0.30).min(1.0);
                self.clear_capture_plan();
                self.update_opponent(killer, adaptability, |estimate, amount| {
                    estimate.aggression += (1.0 - estimate.aggression) * amount;
                });
            }
            NpcEvent::OwnCapture { area } => {
                self.confidence = (self.confidence + (area / 150.0).clamp(0.04, 0.18)).min(1.0);
                self.frustration *= 0.7;
                self.clear_capture_plan();
            }
            NpcEvent::CreditedKill { victim } => {
                self.confidence = (self.confidence + 0.20).min(1.0);
                self.update_opponent(Some(victim), adaptability, |estimate, amount| {
                    estimate.aggression += (0.65 - estimate.aggression) * amount;
                });
            }
            NpcEvent::TerritoryStolen { by, area } => {
                self.frustration = (self.frustration + (area / 100.0).clamp(0.08, 0.25)).min(1.0);
                self.clear_capture_plan();
                self.update_opponent(Some(by), adaptability, |estimate, amount| {
                    estimate.aggression += (0.9 - estimate.aggression) * amount;
                });
            }
        }
    }

    fn update_opponent(
        &mut self,
        opponent: Option<CompetitorId>,
        adaptability: f32,
        update: impl FnOnce(&mut OpponentEstimate, f32),
    ) {
        if !self.adapts || adaptability <= 0.0 {
            return;
        }
        let Some(opponent) = opponent else { return };
        let estimate = &mut self.opponents[opponent.index()];
        update(estimate, adaptability.clamp(0.02, 1.0) * 0.25);
        estimate.observations = estimate.observations.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NpcEvent {
    Spawned,
    Died { killer: Option<CompetitorId> },
    OwnCapture { area: f32 },
    CreditedKill { victim: CompetitorId },
    TerritoryStolen { by: CompetitorId, area: f32 },
}

#[derive(Resource, Clone, Debug, Default)]
pub struct NpcEventQueue(pub Vec<(CompetitorId, NpcEvent)>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_and_commitment_drive_reaction_without_changing_speed() {
        let mut weak = test_traits();
        weak.skill = 0.0;
        weak.commitment = 1.0;
        let mut strong = test_traits();
        strong.skill = 1.0;
        strong.commitment = 0.0;
        assert_eq!(weak.think_hz(), 4.0);
        assert_eq!(strong.think_hz(), 12.0);
        assert!(weak.minimum_commitment() > strong.minimum_commitment());
        assert!(weak.error_radians() > strong.error_radians());
    }

    #[test]
    fn capture_plan_lifecycle_is_explicit() {
        let mut memory = NpcMatchMemory::new(true);
        let plan = CapturePlan {
            waypoints: [Vec2::X, Vec2::splat(4.0), Vec2::new(0.0, 4.0), Vec2::Y],
            estimated_area: 24.0,
            risk_budget: 0.7,
        };
        memory.begin_capture(plan);
        assert_eq!(memory.active_waypoint(), Some(Vec2::X));
        memory.advance_waypoint_if_reached(Vec2::X, 0.1);
        assert_eq!(memory.active_waypoint(), Some(Vec2::splat(4.0)));
        memory.on_event(NpcEvent::OwnCapture { area: 24.0 }, 0.5);
        assert_eq!(memory.active_waypoint(), None);
    }

    #[test]
    fn non_adaptive_memory_ignores_opponents() {
        let mut memory = NpcMatchMemory::new(false);
        memory.on_event(
            NpcEvent::Died {
                killer: Some(CompetitorId(2)),
            },
            1.0,
        );
        assert_eq!(memory.opponents[2], OpponentEstimate::default());
    }

    #[test]
    fn adaptive_memory_learns_from_personal_events() {
        let mut memory = NpcMatchMemory::new(true);
        memory.on_event(
            NpcEvent::TerritoryStolen {
                by: CompetitorId(2),
                area: 20.0,
            },
            0.8,
        );
        assert!(memory.opponents[2].aggression > 0.0);
        assert_eq!(memory.opponents[2].observations, 1);
    }

    fn test_traits() -> NpcTraits {
        NpcTraits {
            skill: 0.5,
            aggression: 0.5,
            greed: 0.5,
            exploration: 0.5,
            composure: 0.5,
            adaptability: 0.5,
            commitment: 0.5,
            turning_bias: 0.0,
        }
    }
}
