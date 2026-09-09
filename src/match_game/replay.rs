//! Canonical command ingestion and replay serialization.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    ids::{CompetitorId, MAX_COMPETITORS},
    match_game::{Competitor, RosterDescriptor, SimulationClock},
};

use super::interface::{MatchSpec, SteeringCommand};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandRejection {
    Late { current_tick: u64 },
    Duplicate,
    Invalid(&'static str),
}

#[derive(Resource, Clone, Debug, Default)]
pub struct PendingCommands {
    /// Last consumed tick. Tick zero is reserved for the step API shorthand;
    /// explicit replay commands begin at tick one.
    current_tick: u64,
    commands: Vec<SteeringCommand>,
    controlled_players: Option<Vec<CompetitorId>>,
}

impl PendingCommands {
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    pub fn reset(&mut self) {
        self.current_tick = 0;
        self.commands.clear();
        self.controlled_players = None;
    }

    pub fn set_controlled_players(&mut self, roster: &[RosterDescriptor]) {
        self.controlled_players = Some(
            roster
                .iter()
                .enumerate()
                .filter_map(|(index, descriptor)| {
                    matches!(descriptor, RosterDescriptor::Human { .. })
                        .then_some(CompetitorId(index as u8))
                })
                .collect(),
        );
    }

    pub fn enqueue(&mut self, command: SteeringCommand) -> Result<(), CommandRejection> {
        self.validate(command)?;
        if command.tick == 0 {
            return Err(CommandRejection::Invalid(
                "tick zero is reserved for the headless step shorthand",
            ));
        }
        if command.tick <= self.current_tick {
            return Err(CommandRejection::Late {
                current_tick: self.current_tick,
            });
        }
        if self
            .commands
            .iter()
            .any(|existing| existing.tick == command.tick && existing.player == command.player)
        {
            return Err(CommandRejection::Duplicate);
        }
        self.commands.push(command);
        self.commands
            .sort_by_key(|command| (command.tick, command.player, command.sequence));
        Ok(())
    }

    /// Validates the complete batch against an unchanged queue, then commits
    /// it. This is deliberately transactional for network packets/replay
    /// chunks: one bad command cannot leave a partial packet queued.
    pub fn enqueue_batch<I>(&mut self, commands: I) -> Result<(), CommandRejection>
    where
        I: IntoIterator<Item = SteeringCommand>,
    {
        let mut candidate = self.clone();
        for command in commands {
            candidate.enqueue(command)?;
        }
        *self = candidate;
        Ok(())
    }

    fn validate(&self, command: SteeringCommand) -> Result<(), CommandRejection> {
        if usize::from(command.player.0) >= MAX_COMPETITORS {
            return Err(CommandRejection::Invalid(
                "player id is outside the roster limit",
            ));
        }
        let Some(players) = self.controlled_players.as_ref() else {
            return Err(CommandRejection::Invalid("human roster is not configured"));
        };
        if !players.contains(&command.player) {
            return Err(CommandRejection::Invalid(
                "commands may only target human roster slots",
            ));
        }
        let direction = command.direction();
        if !direction.is_finite() || !command.magnitude.is_finite() {
            return Err(CommandRejection::Invalid(
                "command contains a non-finite value",
            ));
        }
        if command.magnitude < 0.0 || command.magnitude > 1.0 {
            return Err(CommandRejection::Invalid("magnitude must be in [0, 1]"));
        }
        if direction.length_squared() > 1.0001 {
            return Err(CommandRejection::Invalid(
                "direction must have length at most one",
            ));
        }
        Ok(())
    }

    pub fn take_for_tick(&mut self, tick: u64) -> Vec<SteeringCommand> {
        self.current_tick = tick;
        let mut due = Vec::new();
        let mut future = Vec::with_capacity(self.commands.len());
        for command in self.commands.drain(..) {
            if command.tick == tick {
                due.push(command);
            } else if command.tick > tick {
                future.push(command);
            }
        }
        self.commands = future;
        due.sort_by_key(|command| (command.player, command.sequence));
        due
    }

    pub fn pending_len(&self) -> usize {
        self.commands.len()
    }
}

/// Consumes exactly one canonical command batch before movement. Commands are
/// retained when no command is supplied for a player, which makes sparse
/// network/replay streams equivalent to a held steering input.
pub(crate) fn consume_commands(
    mut clock: ResMut<SimulationClock>,
    mut pending: ResMut<PendingCommands>,
    mut recording: Option<ResMut<MatchReplay>>,
    mut competitors: Query<(&Competitor, &mut super::SteeringIntent)>,
) {
    // This is the sole authoritative tick boundary. NPC thought runs before
    // this system; human/replay commands therefore win deterministically.
    let tick = clock.0.saturating_add(1);
    let commands = pending.take_for_tick(tick);
    clock.0 = tick;
    if let Some(recording) = recording.as_deref_mut() {
        recording.commands.extend(commands.iter().copied());
    }
    for command in commands {
        if let Some((_, mut intent)) = competitors
            .iter_mut()
            .find(|(competitor, _)| competitor.id == command.player)
        {
            let direction = command.direction();
            intent.desired_direction = direction.normalize_or(intent.desired_direction);
            intent.magnitude = command.magnitude;
            intent.source = super::ControlSource::Replay;
        }
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchReplay {
    pub format_version: u32,
    pub spec: MatchSpec,
    pub commands: Vec<SteeringCommand>,
}

impl MatchReplay {
    pub const FORMAT_VERSION: u32 = 1;

    pub fn new(spec: MatchSpec, commands: Vec<SteeringCommand>) -> Self {
        let mut replay = Self {
            format_version: Self::FORMAT_VERSION,
            spec,
            commands,
        };
        replay
            .commands
            .sort_by_key(|command| (command.tick, command.player, command.sequence));
        replay
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn validate_commands(&self) -> Result<(), CommandRejection> {
        self.spec
            .validate()
            .map_err(|_| CommandRejection::Invalid("replay contains an invalid match spec"))?;
        let mut pending = PendingCommands::default();
        pending.set_controlled_players(&self.spec.roster);
        for command in self.commands.iter().copied() {
            pending.enqueue(command)?;
        }
        Ok(())
    }
}

/// Keeps replay output independent from Bevy entities and iteration order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComparableCompetitor {
    pub id: u8,
    pub position: [f32; 2],
    pub heading: [f32; 2],
    pub alive: bool,
    pub territory_area: f32,
    pub kills: u32,
    pub kill_streak: u32,
    pub deaths: u32,
    pub captures_completed: u32,
    pub respawn_remaining: f32,
    pub spawn_protection_remaining: f32,
    pub trail_length: f32,
    pub trail_fingerprint: u64,
    pub npc_fingerprint: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComparableSnapshot {
    pub tick: u64,
    pub phase: String,
    pub winner: Option<u8>,
    pub competitors: Vec<ComparableCompetitor>,
    pub territory_fingerprint: u64,
}
