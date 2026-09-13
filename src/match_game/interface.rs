//! The device-free interface of the authoritative match simulation.
//!
//! Types in this module are intentionally independent of the application shell. A
//! shell may translate devices into [`SteeringCommand`] values, while a replay
//! or a headless caller can provide the same values directly.

use bevy::prelude::*;
use serde::{Deserialize, Serialize, ser::SerializeStruct};

use crate::{
    config::GameConfig,
    ids::{CompetitorId, MAX_COMPETITORS},
    match_game::rules::MatchRules,
    npc::NpcDifficulty,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlSource {
    Gamepad,
    Mouse,
    Keyboard,
    Npc,
    Replay,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SteeringIntent {
    pub desired_direction: Vec2,
    pub magnitude: f32,
    pub source: ControlSource,
}

impl Default for SteeringIntent {
    fn default() -> Self {
        Self {
            desired_direction: Vec2::Y,
            magnitude: 0.0,
            source: ControlSource::Keyboard,
        }
    }
}

/// A roster slot contains identity and presentation data, never an input
/// device. Device attachment is a shell concern and happens after a match is
/// started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RosterDescriptor {
    Human {
        identity: String,
        display_name: String,
        color_id: u8,
        pattern_id: u8,
    },
    Npc,
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchSpec {
    pub seed: u64,
    pub npc_roster_seed: u64,
    pub config: GameConfig,
    pub roster: Vec<RosterDescriptor>,
    pub npc_difficulty: NpcDifficulty,
    pub rules: MatchRules,
    pub purpose: crate::match_game::MatchPurpose,
    /// Countdown duration in authoritative fixed ticks. Zero means the match
    /// starts running, but readiness is still required by the shell.
    pub countdown_ticks: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecValidationError {
    InvalidRoster,
    InvalidConfig(&'static str),
}

impl MatchSpec {
    pub fn validate(&self) -> Result<(), SpecValidationError> {
        if !(2..=MAX_COMPETITORS).contains(&self.roster.len()) {
            return Err(SpecValidationError::InvalidRoster);
        }
        let c = &self.config;
        let finite = [
            c.fixed_hz as f32,
            c.player_speed,
            c.kill_speed_bonus_per_kill,
            c.kill_speed_bonus_cap,
            c.max_turn_rate_radians,
            c.collision_radius,
            c.trail_width,
            c.cell_size,
            c.starting_territory_radius,
            c.spawn_protection_seconds,
            c.spawn_protection_minimum_seconds,
            c.respawn_base_seconds,
            c.respawn_warning_seconds,
            c.respawn_retry_seconds,
            c.self_trail_exclusion_distance,
            c.trail_sample_distance,
            c.trail_sample_angle_radians,
            c.inward_edge_steer,
            c.spawn_cube_clearance,
            c.spawn_trail_clearance,
            c.spawn_boundary_clearance,
            c.npc_think_hz,
            self.rules.respawn_delay_base_seconds,
        ];
        if !finite.into_iter().all(f32::is_finite)
            || c.respawn_cap_seconds.is_some_and(|cap| !cap.is_finite())
            || self
                .rules
                .respawn_delay_cap_seconds
                .is_some_and(|cap| !cap.is_finite())
        {
            return Err(SpecValidationError::InvalidConfig(
                "config contains a non-finite value",
            ));
        }
        if !(c.fixed_hz >= 1.0 && c.fixed_hz <= 1_000.0) {
            return Err(SpecValidationError::InvalidConfig(
                "fixed_hz is outside [1, 1000]",
            ));
        }
        // BoardGrid allocates a square sample cache. This floor is a resource
        // safety limit, not a gameplay resolution preference.
        if !(c.cell_size >= 0.1 && c.cell_size <= 100.0) {
            return Err(SpecValidationError::InvalidConfig(
                "cell_size is outside [0.1, 100]",
            ));
        }
        if c.player_speed < 0.0
            || c.player_speed > 1_000.0
            || !(c.collision_radius > 0.0 && c.collision_radius <= 100.0)
            || !(c.trail_width > 0.0 && c.trail_width <= 100.0)
            || !(c.npc_think_hz > 0.0 && c.npc_think_hz <= 1_000.0)
            || !(c.max_turn_rate_radians > 0.0 && c.max_turn_rate_radians <= 100.0)
            || !(c.starting_territory_radius > 0.0 && c.starting_territory_radius <= 100.0)
            || !(c.trail_sample_distance > 0.0 && c.trail_sample_distance <= 100.0)
            || !(c.self_trail_exclusion_distance >= 0.0 && c.self_trail_exclusion_distance <= 100.0)
            || !(c.kill_speed_bonus_per_kill >= 0.0 && c.kill_speed_bonus_per_kill <= 100.0)
            || !(c.kill_speed_bonus_cap >= 0.0 && c.kill_speed_bonus_cap <= 100.0)
            || !(c.trail_sample_angle_radians > 0.0
                && c.trail_sample_angle_radians <= std::f32::consts::TAU)
            || c.inward_edge_steer < 0.0
            || c.inward_edge_steer > 1.0
            || c.spawn_cube_clearance < 0.0
            || c.spawn_trail_clearance < 0.0
            || c.spawn_boundary_clearance < 0.0
            || c.respawn_warning_seconds < 0.0
            || !(0.5..=1.0).contains(&c.respawn_retry_seconds)
            || !(1..=100_000).contains(&c.respawn_candidate_batch)
        {
            return Err(SpecValidationError::InvalidConfig(
                "config has an out-of-range tuning value",
            ));
        }
        if c.respawn_cap_seconds
            .is_some_and(|cap| !(0.0..=3_600.0).contains(&cap))
            || !(0.0..=3_600.0).contains(&c.respawn_base_seconds)
            || !(0.0..=3_600.0).contains(&c.spawn_protection_seconds)
            || !(0.0..=3_600.0).contains(&c.respawn_warning_seconds)
            || !(0.0..=3_600.0).contains(&c.spawn_protection_minimum_seconds)
            || c.spawn_protection_minimum_seconds > c.spawn_protection_seconds
            || !(1..=100).contains(&c.victory_territory_percent)
            || !(1..=100).contains(&self.rules.victory_threshold_percent)
            || !(0.0..=3_600.0).contains(&self.rules.respawn_delay_base_seconds)
            || self
                .rules
                .respawn_delay_cap_seconds
                .is_some_and(|cap| !(0.0..=3_600.0).contains(&cap))
            || (self.rules.victory_enabled
                && !(1..=100).contains(&self.rules.victory_threshold_percent))
            || self.countdown_ticks > 360_000
        {
            return Err(SpecValidationError::InvalidConfig(
                "duration, threshold, or countdown is outside its bounds",
            ));
        }
        Ok(())
    }

    pub fn from_config(seed: u64, roster: Vec<RosterDescriptor>, config: &GameConfig) -> Self {
        Self {
            seed,
            npc_roster_seed: seed ^ 0x4e50_4352_4f53_5445,
            config: config.clone(),
            roster,
            npc_difficulty: NpcDifficulty::Normal,
            rules: MatchRules::from(config),
            purpose: crate::match_game::MatchPurpose::Playable,
            countdown_ticks: (config.fixed_hz * 3.0).round() as u64,
        }
    }
}

impl Default for MatchSpec {
    fn default() -> Self {
        let config = GameConfig::default();
        Self::from_config(
            1,
            vec![RosterDescriptor::Npc, RosterDescriptor::Npc],
            &config,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SteeringCommand {
    pub tick: u64,
    pub player: CompetitorId,
    pub desired_direction: [f32; 2],
    pub magnitude: f32,
    /// A stable producer sequence used to diagnose and reject duplicates. The
    /// canonical simulation order is player id, then sequence.
    pub sequence: u64,
}

impl Serialize for SteeringCommand {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("SteeringCommand", 5)?;
        state.serialize_field("tick", &self.tick)?;
        state.serialize_field("player", &self.player.0)?;
        state.serialize_field("desired_direction", &self.desired_direction)?;
        state.serialize_field("magnitude", &self.magnitude)?;
        state.serialize_field("sequence", &self.sequence)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for SteeringCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            tick: u64,
            player: u8,
            desired_direction: [f32; 2],
            magnitude: f32,
            sequence: u64,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            tick: raw.tick,
            player: CompetitorId(raw.player),
            desired_direction: raw.desired_direction,
            magnitude: raw.magnitude,
            sequence: raw.sequence,
        })
    }
}

impl SteeringCommand {
    pub fn new(tick: u64, player: CompetitorId, direction: Vec2, magnitude: f32) -> Self {
        Self {
            tick,
            player,
            desired_direction: direction.to_array(),
            magnitude,
            sequence: 0,
        }
    }

    pub fn direction(self) -> Vec2 {
        Vec2::from_array(self.desired_direction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respawn_tunables_reject_nonfinite_and_unbounded_work() {
        for warning in [-1.0, f32::NAN, f32::INFINITY, 3_601.0] {
            let mut spec = MatchSpec::default();
            spec.config.respawn_warning_seconds = warning;
            assert!(spec.validate().is_err());
        }
        for retry in [0.0, 0.49, 1.01, f32::NAN, f32::INFINITY] {
            let mut spec = MatchSpec::default();
            spec.config.respawn_retry_seconds = retry;
            assert!(spec.validate().is_err());
        }
        for batch in [0, 100_001, usize::MAX] {
            let mut spec = MatchSpec::default();
            spec.config.respawn_candidate_batch = batch;
            assert!(spec.validate().is_err());
        }
        assert!(MatchSpec::default().validate().is_ok());
    }

    #[test]
    fn spec_validation_rejects_bad_rosters_and_unbounded_tunables() {
        let mut spec = MatchSpec::default();
        spec.roster.clear();
        assert_eq!(spec.validate(), Err(SpecValidationError::InvalidRoster));

        let mut spec = MatchSpec::default();
        spec.config.cell_size = 0.001;
        assert!(matches!(
            spec.validate(),
            Err(SpecValidationError::InvalidConfig(_))
        ));

        let mut spec = MatchSpec::default();
        spec.rules.victory_threshold_percent = 0;
        assert!(spec.validate().is_err());
    }
}
