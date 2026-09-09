use bevy::prelude::*;

/// Tunable authoritative match values. Presentation settings intentionally live elsewhere.
#[derive(Resource, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GameConfig {
    pub fixed_hz: f64,
    pub player_speed: f32,
    /// Fractional speed added for each credited kill, before the match cap.
    pub kill_speed_bonus_per_kill: f32,
    /// Maximum total speed bonus earned from kills.
    pub kill_speed_bonus_cap: f32,
    pub max_turn_rate_radians: f32,
    pub collision_radius: f32,
    pub trail_width: f32,
    pub cell_size: f32,
    pub starting_territory_radius: f32,
    pub spawn_protection_seconds: f32,
    pub spawn_protection_minimum_seconds: f32,
    pub respawn_base_seconds: f32,
    pub respawn_cap_seconds: Option<f32>,
    /// Whole-arena territory share required to win. The comparison is made
    /// against authoritative fixed-point polygon area, not displayed text.
    pub victory_territory_percent: u8,
    pub self_trail_exclusion_distance: f32,
    pub trail_sample_distance: f32,
    pub trail_sample_angle_radians: f32,
    pub inward_edge_steer: f32,
    pub spawn_cube_clearance: f32,
    pub spawn_trail_clearance: f32,
    pub spawn_boundary_clearance: f32,
    pub npc_think_hz: f32,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            fixed_hz: 60.0,
            player_speed: 8.0,
            kill_speed_bonus_per_kill: 0.035,
            kill_speed_bonus_cap: 0.28,
            max_turn_rate_radians: 270.0_f32.to_radians(),
            collision_radius: 0.52,
            trail_width: 0.65,
            cell_size: 0.5,
            starting_territory_radius: 2.75,
            spawn_protection_seconds: 1.25,
            spawn_protection_minimum_seconds: 0.5,
            // Keep a cut costly without turning a local party game into spectating.
            respawn_base_seconds: 2.0,
            respawn_cap_seconds: Some(5.0),
            victory_territory_percent: 95,
            self_trail_exclusion_distance: 1.5,
            trail_sample_distance: 0.2,
            trail_sample_angle_radians: 6.0_f32.to_radians(),
            inward_edge_steer: 0.2,
            spawn_cube_clearance: 12.0,
            spawn_trail_clearance: 6.0,
            spawn_boundary_clearance: 2.75,
            npc_think_hz: 8.0,
        }
    }
}

impl GameConfig {
    pub fn fixed_delta_seconds(&self) -> f32 {
        (1.0 / self.fixed_hz) as f32
    }

    pub fn speed_multiplier_for_kills(&self, kills: u32) -> f32 {
        1.0 + (self.kill_speed_bonus_per_kill.max(0.0) * kills as f32)
            .min(self.kill_speed_bonus_cap.max(0.0))
    }

    pub fn player_speed_for_kills(&self, kills: u32) -> f32 {
        self.player_speed * self.speed_multiplier_for_kills(kills)
    }

    pub fn respawn_delay(&self, deaths: u32) -> f32 {
        let delay = self.respawn_base_seconds * deaths as f32;
        self.respawn_cap_seconds.map_or(delay, |cap| delay.min(cap))
    }

    /// Converts an authoritative arena percentage into the normalized value
    /// shown in player-facing territory displays. Winning territory maps to
    /// 100%, and malformed or over-cap values cannot exceed that maximum.
    pub fn display_territory_percent(&self, actual_percent: f32) -> f32 {
        let threshold = f32::from(self.victory_territory_percent);
        if threshold <= 0.0 || !actual_percent.is_finite() {
            0.0
        } else {
            (actual_percent * 100.0 / threshold).clamp(0.0, 100.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respawn_delay_escalates_and_can_be_capped() {
        let mut config = GameConfig::default();
        assert_eq!([1, 2, 3].map(|n| config.respawn_delay(n)), [2.0, 4.0, 5.0]);
        assert_eq!(config.respawn_delay(100), 5.0);
        config.respawn_cap_seconds = None;
        assert_eq!(config.respawn_delay(3), 6.0);
        config.respawn_cap_seconds = Some(3.0);
        assert_eq!(config.respawn_delay(3), 3.0);
    }

    #[test]
    fn display_territory_percent_normalizes_the_victory_threshold() {
        let config = GameConfig::default();
        assert_eq!(config.display_territory_percent(95.0), 100.0);
        assert_eq!(config.display_territory_percent(47.5), 50.0);
        assert_eq!(config.display_territory_percent(100.0), 100.0);
        assert_eq!(config.display_territory_percent(-1.0), 0.0);
        assert_eq!(config.display_territory_percent(f32::NAN), 0.0);
    }

    #[test]
    fn kill_speed_bonus_is_small_progressive_and_capped() {
        let config = GameConfig::default();
        assert_eq!(config.speed_multiplier_for_kills(0), 1.0);
        assert!((config.speed_multiplier_for_kills(2) - 1.07).abs() < 1.0e-6);
        assert_eq!(config.speed_multiplier_for_kills(100), 1.28);

        let config = GameConfig {
            kill_speed_bonus_per_kill: -1.0,
            kill_speed_bonus_cap: -1.0,
            ..GameConfig::default()
        };
        assert_eq!(config.speed_multiplier_for_kills(10), 1.0);
    }
}
