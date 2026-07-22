use bevy::prelude::*;

/// Tunable authoritative match values. Presentation settings intentionally live elsewhere.
#[derive(Resource, Clone, Debug)]
pub struct GameConfig {
    pub fixed_hz: f64,
    pub player_speed: f32,
    pub max_turn_rate_radians: f32,
    pub collision_radius: f32,
    pub trail_width: f32,
    pub cell_size: f32,
    pub starting_territory_radius: f32,
    pub spawn_protection_seconds: f32,
    pub spawn_protection_minimum_seconds: f32,
    pub respawn_base_seconds: f32,
    pub respawn_cap_seconds: Option<f32>,
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
            max_turn_rate_radians: 270.0_f32.to_radians(),
            collision_radius: 0.52,
            trail_width: 0.65,
            cell_size: 0.5,
            starting_territory_radius: 2.75,
            spawn_protection_seconds: 1.25,
            spawn_protection_minimum_seconds: 0.5,
            respawn_base_seconds: 5.0,
            respawn_cap_seconds: None,
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

    pub fn respawn_delay(&self, deaths: u32) -> f32 {
        let delay = self.respawn_base_seconds * deaths as f32;
        self.respawn_cap_seconds.map_or(delay, |cap| delay.min(cap))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respawn_delay_escalates_and_can_be_capped() {
        let mut config = GameConfig::default();
        assert_eq!(
            [1, 2, 3].map(|n| config.respawn_delay(n)),
            [5.0, 10.0, 15.0]
        );
        config.respawn_cap_seconds = Some(12.0);
        assert_eq!(config.respawn_delay(3), 12.0);
    }
}
