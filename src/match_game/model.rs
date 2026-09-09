use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ids::CompetitorId;

#[derive(Component, Clone, Debug)]
pub struct Competitor {
    pub id: CompetitorId,
    pub display_name: String,
    pub kind: CompetitorKind,
    pub color_id: u8,
    pub pattern_id: u8,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompetitorKind {
    Human,
    Npc,
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct LifeState {
    pub status: LifeStatus,
    pub respawn_remaining: f32,
}
impl LifeState {
    pub fn alive() -> Self {
        Self {
            status: LifeStatus::Alive,
            respawn_remaining: 0.0,
        }
    }
    pub fn is_alive(self) -> bool {
        self.status == LifeStatus::Alive
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifeStatus {
    Alive,
    Respawning,
    Eliminated,
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct SpawnProtection {
    pub remaining: f32,
    pub elapsed: f32,
}
impl SpawnProtection {
    pub fn active(self) -> bool {
        self.remaining > 0.0
    }
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct TerritoryRecord {
    /// Exact vector area in world units.
    pub current_area: f32,
    pub peak_area: f32,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct MatchStatistics {
    pub kills: u32,
    pub kill_streak: u32,
    pub best_kill_streak: u32,
    pub deaths: u32,
    pub captures_completed: u32,
    pub area_captured_total: f32,
    pub area_stolen_total: f32,
    pub largest_capture_area: f32,
    pub peak_territory_area: f32,
    pub longest_trail_length: f32,
    pub time_alive_seconds: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KillProgress {
    pub total: u32,
    pub streak: u32,
}

impl MatchStatistics {
    pub fn record_kill(&mut self) -> KillProgress {
        self.kills = self.kills.saturating_add(1);
        self.kill_streak = self.kill_streak.saturating_add(1);
        self.best_kill_streak = self.best_kill_streak.max(self.kill_streak);
        KillProgress {
            total: self.kills,
            streak: self.kill_streak,
        }
    }

    pub fn reset_kill_streak(&mut self) {
        self.kill_streak = 0;
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub struct LastOwnedCell(pub crate::board::Cell);

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct MatchSession {
    pub field_seed: u64,
    pub npc_roster_seed: u64,
    pub purpose: MatchPurpose,
    pub phase: MatchPhase,
    pub elapsed_seconds: f32,
    /// Display-friendly seconds, derived from the authoritative tick count.
    pub countdown_remaining: f32,
    pub countdown_ticks_remaining: u64,
    pub winner: Option<CompetitorId>,
    pub result_hold_remaining: f32,
}
impl Default for MatchSession {
    fn default() -> Self {
        Self {
            field_seed: 0,
            npc_roster_seed: 0,
            purpose: MatchPurpose::Playable,
            phase: MatchPhase::Idle,
            elapsed_seconds: 0.0,
            countdown_remaining: 3.0,
            countdown_ticks_remaining: 0,
            winner: None,
            result_hold_remaining: 0.0,
        }
    }
}

/// Describes who owns the match lifecycle. The authoritative simulation is
/// shared by playable matches and the Home attract match; only their shell
/// behavior differs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum MatchPurpose {
    #[default]
    Playable,
    Attract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MatchPhase {
    Idle,
    /// The match world exists, but its generation has not been acknowledged
    /// by presentation yet.
    Loading,
    Countdown,
    Running,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RankingEntry {
    pub id: CompetitorId,
    pub rank: u8,
    pub territory_area: f32,
    pub territory_percent: f32,
    pub alive: bool,
    pub kills: u32,
}
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Rankings {
    pub entries: Vec<RankingEntry>,
}
impl Rankings {
    pub fn rank_of(&self, id: CompetitorId) -> Option<&RankingEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
    pub fn leader(&self) -> Option<CompetitorId> {
        self.entries.first().filter(|e| e.alive).map(|e| e.id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathCause {
    TrailCut,
    SelfTrail,
    Displaced,
}
#[derive(Clone, Debug, PartialEq)]
pub enum SimulationEvent {
    Countdown(u8),
    Go,
    TrailStarted {
        player: CompetitorId,
    },
    Capture {
        player: CompetitorId,
        area: f32,
        stolen_area: f32,
        loop_fill: bool,
    },
    Death {
        victim: CompetitorId,
        killer: Option<CompetitorId>,
        cause: DeathCause,
    },
    Kill {
        killer: CompetitorId,
        progress: KillProgress,
    },
    Respawn {
        player: CompetitorId,
    },
    RankingChanged,
    Victory {
        winner: CompetitorId,
    },
}
#[derive(Resource, Clone, Debug, Default)]
pub struct SimulationEvents(pub Vec<SimulationEvent>);
impl SimulationEvents {
    pub fn drain(&mut self) -> impl Iterator<Item = SimulationEvent> + '_ {
        self.0.drain(..)
    }
}

/// Small authoritative history consumed by the HUD. Unlike `SimulationEvents`,
/// entries remain available after presentation systems have handled the event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EliminationRecord {
    pub victim: CompetitorId,
    pub killer: Option<CompetitorId>,
    pub cause: DeathCause,
    pub match_time: f32,
}

#[derive(Resource, Clone, Debug, Default)]
pub struct EliminationFeed(pub Vec<EliminationRecord>);

impl EliminationFeed {
    pub const CAPACITY: usize = 5;

    pub fn push(&mut self, record: EliminationRecord) {
        if self.0.len() == Self::CAPACITY {
            self.0.remove(0);
        }
        self.0.push(record);
    }
}

#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MatchSystemSet {
    PollInput,
    NpcThink,
    BuildSteeringIntent,
    MoveCompetitors,
    ExtendTrails,
    DetectTrailCollisions,
    ResolveDeaths,
    DetectClosures,
    ResolveCaptures,
    ResolveTerritoryConsequences,
    CheckVictory,
    AdvanceRespawns,
    UpdateRankings,
}

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatchGeneration(pub u64);

/// The renderer acknowledges a generation only after its pipeline and match
/// world are ready. A stale acknowledgement can never unblock a new match.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PresentationReady(pub Option<u64>);

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimulationPaused(pub bool);

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimulationClock(pub u64);

#[derive(Resource, Default)]
pub(crate) struct PendingDeaths(pub Vec<crate::combat::TrailCollisionIntent>);
#[derive(Clone)]
pub(crate) struct PendingCapture {
    pub player: CompetitorId,
    pub entity: Entity,
    pub time: f32,
    pub trail: crate::trail::ActiveTrail,
}
#[derive(Resource, Default)]
pub(crate) struct PendingCaptures(pub Vec<PendingCapture>);
#[derive(Resource, Default)]
pub(crate) struct DisplacementCredits(pub Vec<(CompetitorId, CompetitorId)>);
