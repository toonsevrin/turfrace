use crate::config::GameConfig;
use serde::{Deserialize, Serialize};

/// Authoritative match policy, kept separate from rendering and shell state.
#[derive(bevy::prelude::Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchRules {
    pub victory_enabled: bool,
    pub victory_threshold_percent: u8,
    pub respawn_enabled: bool,
    pub respawn_delay_base_seconds: f32,
    pub respawn_delay_cap_seconds: Option<f32>,
    pub scoring_enabled: bool,
}

impl MatchRules {
    pub fn respawn_delay(&self, deaths: u32) -> f32 {
        let delay = self.respawn_delay_base_seconds * deaths as f32;
        self.respawn_delay_cap_seconds
            .map_or(delay, |cap| delay.min(cap))
    }
}

impl From<&GameConfig> for MatchRules {
    fn from(config: &GameConfig) -> Self {
        Self {
            victory_enabled: true,
            victory_threshold_percent: config.victory_territory_percent,
            respawn_enabled: true,
            respawn_delay_base_seconds: config.respawn_base_seconds,
            respawn_delay_cap_seconds: config.respawn_cap_seconds,
            scoring_enabled: true,
        }
    }
}

impl Default for MatchRules {
    fn default() -> Self {
        Self::from(&GameConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_mirror_game_config_and_delay_escalates() {
        let config = GameConfig::default();
        let rules = MatchRules::from(&config);
        assert_eq!(rules.victory_threshold_percent, 95);
        assert_eq!(rules.respawn_delay(1), 2.0);
        assert_eq!(rules.respawn_delay(3), 5.0);
    }

    #[test]
    fn policy_can_disable_victory_respawn_and_scoring() {
        let rules = MatchRules {
            victory_enabled: false,
            respawn_enabled: false,
            scoring_enabled: false,
            ..MatchRules::default()
        };
        assert!(!rules.victory_enabled);
        assert!(!rules.respawn_enabled);
        assert!(!rules.scoring_enabled);
    }
}
