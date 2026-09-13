use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    board::{BoardGrid, Cell},
    config::GameConfig,
    ids::{CompetitorId, MAX_COMPETITORS},
    territory_map::TerritoryMap,
};

pub const NPC_ROUTE_CAPACITY: usize = 8;
pub const NPC_VISIBLE_SEGMENT_CAP: usize = 24;
pub const NPC_TRAIL_SNAPSHOT_CAP: usize = 128;
/// Competitor count is the bounded rival horizon: no owner can be starved by
/// a nearest-four truncation when the arena is crowded.
pub const NPC_VISIBLE_RIVAL_CAP: usize = MAX_COMPETITORS;
pub const NPC_RELEVANT_TRAIL_SEGMENT_CAP: usize = 64;
// Kept as a source-compatible name for integrations that display route sizes.
pub const NPC_WAYPOINT_CAPACITY: usize = NPC_ROUTE_CAPACITY;

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
        Self::ALL[(self as i8 + delta).rem_euclid(3) as usize]
    }
    /// Bounds for control/awareness/judgment/composure, not policy selection.
    pub const fn competence_bounds(self) -> (f32, f32) {
        match self {
            Self::Easy => (0.20, 0.62),
            Self::Normal => (0.42, 0.82),
            Self::Hard => (0.66, 0.98),
        }
    }
    pub const fn skill_bounds(self) -> (f32, f32) {
        self.competence_bounds()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TurnSide {
    Left,
    Right,
}
impl TurnSide {
    pub fn sign(self) -> f32 {
        match self {
            Self::Left => 1.0,
            Self::Right => -1.0,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BuilderShape {
    Fill,
    Seal,
    BroadSweep,
    Roamer,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HunterTarget {
    Trail,
    ExposedRival,
    Opportunistic,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RaidObjective {
    Leader,
    WeakestBorder,
    TrailCut,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RaidShape {
    Hook,
    Wedge,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BuilderPolicy {
    pub shape: BuilderShape,
    pub side: TurnSide,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HunterPolicy {
    pub target: HunterTarget,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RaiderPolicy {
    pub objective: RaidObjective,
    pub shape: RaidShape,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NpcPolicy {
    Builder(BuilderPolicy),
    Hunter(HunterPolicy),
    Raider(RaiderPolicy),
}
impl NpcPolicy {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Builder(_) => "Builder",
            Self::Hunter(_) => "Hunter",
            Self::Raider(_) => "Raider",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcCompetence {
    /// The only execution parameter. Policies own the reason, risk, and size
    /// of an action; skill only controls how reliably that action is executed.
    pub skill: f32,
}
impl NpcCompetence {
    pub fn skill(self) -> f32 {
        self.skill.clamp(0.0, 1.0)
    }
    pub fn sensor_horizon(self) -> f32 {
        (16.0 + self.skill() * 14.0).clamp(16.0, 30.0)
    }
    pub fn think_hz(self) -> f32 {
        6.0 + self.skill() * 6.0
    }
    pub fn reaction_horizon(self) -> f32 {
        (0.28 + self.skill() * 0.52).clamp(0.28, 0.8)
    }
    pub fn forecast_horizon(self) -> f32 {
        (0.35 + self.skill() * 0.85).clamp(0.35, 1.2)
    }
    pub fn mistake_chance(self) -> f32 {
        (0.16 * (1.0 - self.skill())).clamp(0.015, 0.16)
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcProfile {
    pub policy: NpcPolicy,
    pub competence: NpcCompetence,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReturnReason {
    #[default]
    Safety,
    TrailLimit,
    Threat,
    CaptureComplete,
    Recovery,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapturePurpose {
    FillFrontier,
    SealGap,
    CutTrail,
    RaidBorder,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TacticPhase {
    Acquiring,
    Travelling,
    Committing,
    Aborting,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HuntTarget {
    Segment { owner: CompetitorId, segment: usize },
    Rival(CompetitorId),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RaidTarget {
    Border { owner: CompetitorId, point: Vec2 },
    Leader(CompetitorId),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NpcTacticKind {
    Return(ReturnReason),
    Capture(CapturePurpose),
    Hunt(HuntTarget),
    Raid(RaidTarget),
    Roam,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteTarget {
    OwnedGround,
    Frontier,
    Segment {
        owner: CompetitorId,
        index: usize,
    },
    Rival(CompetitorId),
    EnemyBorder(CompetitorId),
    OpenSpace,
    /// No owned endpoint was reachable. This is intentionally distinct from
    /// `OwnedGround`: callers must keep planning rather than report success.
    EmergencyReturn,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcRoute {
    pub points: [Vec2; NPC_ROUTE_CAPACITY],
    pub count: u8,
    pub target: RouteTarget,
}
impl Default for NpcRoute {
    fn default() -> Self {
        Self {
            points: [Vec2::ZERO; NPC_ROUTE_CAPACITY],
            count: 0,
            target: RouteTarget::OpenSpace,
        }
    }
}
impl NpcRoute {
    pub fn from_points(points: &[Vec2], target: RouteTarget) -> Self {
        let mut route = Self {
            target,
            ..default()
        };
        route.count = points.len().min(NPC_ROUTE_CAPACITY) as u8;
        route.points[..route.count as usize].copy_from_slice(&points[..route.count as usize]);
        route
    }
    pub fn active(&self, index: u8) -> Option<Vec2> {
        (index < self.count).then(|| self.points[index as usize])
    }
    pub fn final_point(&self) -> Option<Vec2> {
        self.active(self.count.saturating_sub(1))
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NpcMistake {
    #[default]
    LateAbort,
    MisreadIntercept,
    OvercommitReturn,
    PoorSideChoice,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncounterOutcome {
    Completed,
    Abandoned,
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcVisibleRival {
    pub id: CompetitorId,
    pub relative: Vec2,
    pub distance: f32,
    pub territory_share: f32,
    pub exposed: bool,
    pub heading: Vec2,
    pub speed: f32,
}
impl Default for NpcVisibleRival {
    fn default() -> Self {
        Self {
            id: CompetitorId(0),
            relative: Vec2::ZERO,
            distance: 0.0,
            territory_share: 0.0,
            exposed: false,
            heading: Vec2::Y,
            speed: 0.0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcVisibleSegment {
    pub owner: CompetitorId,
    pub segment: usize,
    pub start: Vec2,
    pub end: Vec2,
    pub nearest_point: Vec2,
    pub relative: Vec2,
    pub distance: f32,
    pub tangent: Vec2,
    pub own: bool,
}
impl Default for NpcVisibleSegment {
    fn default() -> Self {
        Self {
            owner: CompetitorId(0),
            segment: 0,
            start: Vec2::ZERO,
            end: Vec2::X,
            nearest_point: Vec2::ZERO,
            relative: Vec2::ZERO,
            distance: 0.0,
            tangent: Vec2::X,
            own: false,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NpcEncounter {
    pub target: Option<HuntTarget>,
    pub position: Vec2,
    pub velocity: Vec2,
    pub distance: f32,
    pub intercept_time: f32,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NpcMemoryEvent {
    pub tick: u64,
    pub opponent: Option<CompetitorId>,
    pub outcome: Option<EncounterOutcome>,
    pub location: Vec2,
    pub area: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OpponentMemory {
    pub observations: u16,
    pub threat: f32,
    pub last_seen_tick: u64,
    pub last_outcome: Option<EncounterOutcome>,
    pub last_location: Vec2,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FailureMemory {
    pub tick: u64,
    pub mistake: NpcMistake,
    pub location: Vec2,
}
#[derive(Clone, Debug)]
pub struct NpcEventMemory {
    pub recent: [NpcMemoryEvent; 8],
    pub recent_len: u8,
    pub opponents: [OpponentMemory; MAX_COMPETITORS],
    pub last_failure: Option<FailureMemory>,
    pub revenge_target: Option<CompetitorId>,
    pub revenge_expires: u64,
    pub revenge_used: bool,
    pub confidence: f32,
    pub frustration: f32,
    pub planned_capture_area: f32,
    pub decision_counter: u64,
    pub pursuit_cooldown_until: u64,
}
impl Default for NpcEventMemory {
    fn default() -> Self {
        Self::new()
    }
}
impl NpcEventMemory {
    pub fn new() -> Self {
        Self {
            recent: [NpcMemoryEvent::default(); 8],
            recent_len: 0,
            opponents: [OpponentMemory::default(); MAX_COMPETITORS],
            last_failure: None,
            revenge_target: None,
            revenge_expires: 0,
            revenge_used: false,
            confidence: 0.5,
            frustration: 0.0,
            planned_capture_area: 0.0,
            decision_counter: 0,
            pursuit_cooldown_until: 0,
        }
    }
    pub fn advance(&mut self, seconds: f32) {
        self.frustration *= (-seconds.max(0.0) * 0.12).exp();
    }
    pub fn record(&mut self, event: NpcMemoryEvent) {
        if self.recent_len < 8 {
            self.recent[self.recent_len as usize] = event;
            self.recent_len += 1;
        } else {
            self.recent.rotate_left(1);
            self.recent[7] = event;
        }
    }
    pub fn record_encounter(
        &mut self,
        opponent: Option<CompetitorId>,
        outcome: EncounterOutcome,
        tick: u64,
        location: Vec2,
        confidence: f32,
    ) {
        self.record(NpcMemoryEvent {
            tick,
            opponent,
            outcome: Some(outcome),
            location,
            area: confidence.clamp(0.0, 1.0),
        });
        if let Some(id) = opponent {
            let e = &mut self.opponents[id.index()];
            e.observations = e.observations.saturating_add(1);
            e.last_seen_tick = tick;
            e.last_outcome = Some(outcome);
            e.last_location = location;
        }
    }
    pub fn on_event(&mut self, event: NpcEvent, tick: u64) {
        let (opponent, area, outcome, location) = match event {
            NpcEvent::Spawned => (None, 0.0, None, Vec2::ZERO),
            NpcEvent::Died { killer } => {
                (killer, 0.0, Some(EncounterOutcome::Abandoned), Vec2::ZERO)
            }
            NpcEvent::OwnCapture { area } => {
                (None, area, Some(EncounterOutcome::Completed), Vec2::ZERO)
            }
            NpcEvent::CreditedKill { victim } => (
                Some(victim),
                0.0,
                Some(EncounterOutcome::Completed),
                Vec2::ZERO,
            ),
            NpcEvent::TerritoryStolen { by, area, location } => {
                (Some(by), area, Some(EncounterOutcome::Abandoned), location)
            }
            NpcEvent::EncounterCompleted { opponent } => {
                (opponent, 0.0, Some(EncounterOutcome::Completed), Vec2::ZERO)
            }
            NpcEvent::EncounterAbandoned { opponent } => {
                (opponent, 0.0, Some(EncounterOutcome::Abandoned), Vec2::ZERO)
            }
        };
        self.record(NpcMemoryEvent {
            tick,
            opponent,
            outcome,
            location,
            area,
        });
        match event {
            NpcEvent::Spawned => {
                self.confidence = 0.5;
                self.frustration = 0.0;
                self.revenge_target = None;
            }
            NpcEvent::Died { killer } => {
                self.confidence = (self.confidence - 0.22).max(0.0);
                self.frustration = (self.frustration + 0.30).min(1.0);
                self.start_revenge(killer, tick, 900);
            }
            NpcEvent::OwnCapture { area } => {
                self.confidence = (self.confidence + (area / 150.0).clamp(0.04, 0.18)).min(1.0);
                self.planned_capture_area = 0.0;
            }
            NpcEvent::CreditedKill { victim } => {
                self.confidence = (self.confidence + 0.20).min(1.0);
                self.revenge_target = if self.revenge_target == Some(victim) {
                    None
                } else {
                    self.revenge_target
                };
            }
            NpcEvent::TerritoryStolen { by, area, .. } => {
                self.frustration = (self.frustration + (area / 100.0).clamp(0.08, 0.25)).min(1.0);
                self.start_revenge(Some(by), tick, 720);
            }
            NpcEvent::EncounterCompleted { .. } | NpcEvent::EncounterAbandoned { .. } => {}
        }
        if let Some(id) = opponent {
            let e = &mut self.opponents[id.index()];
            e.observations = e.observations.saturating_add(1);
            e.last_seen_tick = tick;
            e.last_outcome = outcome;
            e.last_location = location;
            e.threat = (e.threat
                + if matches!(outcome, Some(EncounterOutcome::Abandoned)) {
                    0.2
                } else {
                    0.05
                })
            .clamp(0.0, 1.0);
        }
    }
    fn start_revenge(&mut self, target: Option<CompetitorId>, tick: u64, duration: u64) {
        // A new relevant outcome gets one bounded revenge opportunity. It must
        // not be a permanent global target, but a previous event must not block
        // the next event either.
        self.revenge_used = false;
        self.revenge_target = target;
        self.revenge_expires = tick.saturating_add(duration);
    }
    pub fn revenge_available(&self, tick: u64, id: CompetitorId) -> bool {
        self.revenge_target == Some(id) && !self.revenge_used && tick <= self.revenge_expires
    }
    pub fn use_revenge(&mut self) {
        self.revenge_used = true;
        self.revenge_target = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcTactic {
    pub kind: NpcTacticKind,
    pub phase: TacticPhase,
    pub route: NpcRoute,
    pub route_index: u8,
    pub started_tick: u64,
    pub next_interrupt_tick: u64,
    pub max_duration_ticks: u32,
    pub mistake: Option<NpcMistake>,
    pub encounter: NpcEncounter,
    /// A route may only complete after leaving owned ground and observing
    /// re-entry (or after the simulation reports its capture/kill).
    pub left_owned: bool,
}
impl NpcTactic {
    pub fn new(kind: NpcTacticKind, route: NpcRoute, tick: u64) -> Self {
        Self {
            kind,
            phase: TacticPhase::Acquiring,
            route,
            route_index: 0,
            started_tick: tick,
            next_interrupt_tick: tick + 12,
            max_duration_ticks: 240,
            mistake: None,
            encounter: NpcEncounter::default(),
            left_owned: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcObservation {
    pub position: Vec2,
    pub heading: Vec2,
    pub speed: f32,
    pub protected: bool,
    pub owns_current_cell: bool,
    pub trail_length: f32,
    pub edge_distance: f32,
    pub inward_direction: Vec2,
    pub home: Option<Vec2>,
    pub rivals: [Option<NpcVisibleRival>; NPC_VISIBLE_RIVAL_CAP],
    pub segments: [Option<NpcVisibleSegment>; NPC_VISIBLE_SEGMENT_CAP],
    pub encounter: NpcEncounter,
}
#[derive(Clone, Copy, Debug)]
pub struct NpcTickContext<'a> {
    pub board: &'a BoardGrid,
    pub territory: &'a TerritoryMap,
    pub config: &'a GameConfig,
    pub rank: u8,
    pub tick: u64,
    pub speed: f32,
    pub last_owned: Cell,
    /// Exact active trail, when present. Planning uses it for swept self
    /// collision rather than treating a route as safe from sampled points.
    pub own_trail: Option<&'a crate::trail::ActiveTrail>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NpcEvent {
    Spawned,
    Died {
        killer: Option<CompetitorId>,
    },
    OwnCapture {
        area: f32,
    },
    CreditedKill {
        victim: CompetitorId,
    },
    TerritoryStolen {
        by: CompetitorId,
        area: f32,
        location: Vec2,
    },
    EncounterCompleted {
        opponent: Option<CompetitorId>,
    },
    EncounterAbandoned {
        opponent: Option<CompetitorId>,
    },
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn event_memory_is_bounded_and_revenge_is_single_use() {
        let mut memory = NpcEventMemory::new();
        for tick in 0..20 {
            memory.on_event(
                NpcEvent::TerritoryStolen {
                    by: CompetitorId(2),
                    area: 1.0,
                    location: Vec2::new(tick as f32, 2.0),
                },
                tick,
            );
        }
        assert_eq!(memory.recent_len, 8);
        assert!(memory.revenge_available(19, CompetitorId(2)));
        memory.use_revenge();
        assert!(!memory.revenge_available(19, CompetitorId(2)));
    }
    #[test]
    fn revenge_expires_and_keeps_stolen_border_location() {
        let mut memory = NpcEventMemory::new();
        memory.on_event(
            NpcEvent::TerritoryStolen {
                by: CompetitorId(1),
                area: 2.0,
                location: Vec2::new(3.0, 4.0),
            },
            10,
        );
        assert!(!memory.revenge_available(911, CompetitorId(1)));
        assert_eq!(memory.recent[0].location, Vec2::new(3.0, 4.0));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcEventMessage {
    pub recipient: CompetitorId,
    pub event: NpcEvent,
    pub tick: u64,
}
#[derive(Resource, Clone, Debug, Default)]
pub struct NpcEventQueue(pub Vec<NpcEventMessage>);
