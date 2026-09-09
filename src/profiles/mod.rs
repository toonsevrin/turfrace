//! Browser-local player identity, settings, and lifetime statistics.

#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const PROFILE_SCHEMA_VERSION: u32 = 2;
pub const MAX_PROFILE_NAME_CHARS: usize = 16;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LifetimeStatistics {
    pub games_played: u32,
    pub wins: u32,
    pub kills: u32,
    pub deaths: u32,
    pub total_captured_area: f32,
    pub best_territory_percent: f32,
    pub largest_capture_percent: f32,
    pub longest_trail_world_units: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalProfile {
    pub schema_version: u32,
    pub id: String,
    pub display_name: String,
    pub icon_id: u8,
    pub preferred_color_id: u8,
    pub created_at_unix_ms: u64,
    pub last_used_at_unix_ms: u64,
    #[serde(default)]
    pub statistics: LifetimeStatistics,
}

impl LocalProfile {
    pub fn temporary(player_number: usize) -> Self {
        let now = unix_time_ms();
        Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            id: format!("local-{now}-{player_number}"),
            display_name: format!("Player {player_number}"),
            icon_id: ((player_number - 1) % 8) as u8,
            preferred_color_id: ((player_number - 1) % 12) as u8,
            created_at_unix_ms: now,
            last_used_at_unix_ms: now,
            statistics: LifetimeStatistics::default(),
        }
    }

    pub fn rename(&mut self, requested: &str) {
        self.display_name = sanitize_profile_name(requested, &self.display_name);
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct ProfileStore {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub profiles: Vec<LocalProfile>,
}

impl Default for ProfileStore {
    fn default() -> Self {
        Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            profiles: Vec::new(),
        }
    }
}

impl ProfileStore {
    pub fn create(&mut self, requested_name: &str) -> &LocalProfile {
        let number = self.profiles.len() + 1;
        let mut profile = LocalProfile::temporary(number);
        profile.display_name = sanitize_profile_name(requested_name, &profile.display_name);
        self.profiles.push(profile);
        self.profiles.last().expect("profile was just inserted")
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.profiles.len();
        self.profiles.retain(|profile| profile.id != id);
        before != self.profiles.len()
    }

    pub fn reset_statistics(&mut self) {
        for profile in &mut self.profiles {
            profile.statistics = LifetimeStatistics::default();
        }
    }

    pub fn sorted_leaderboard(&self) -> Vec<&LocalProfile> {
        let mut profiles: Vec<_> = self.profiles.iter().collect();
        profiles.sort_by(|a, b| {
            b.statistics
                .wins
                .cmp(&a.statistics.wins)
                .then_with(|| {
                    b.statistics
                        .best_territory_percent
                        .total_cmp(&a.statistics.best_territory_percent)
                })
                .then_with(|| b.statistics.kills.cmp(&a.statistics.kills))
                .then_with(|| b.statistics.games_played.cmp(&a.statistics.games_played))
                .then_with(|| a.display_name.cmp(&b.display_name))
        });
        profiles
    }
}

fn schema_version() -> u32 {
    PROFILE_SCHEMA_VERSION
}

#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    pub schema_version: u32,
    pub master_volume: f32,
    pub music_volume: f32,
    pub sound_effect_volume: f32,
    pub screen_shake: f32,
    pub reduced_motion: bool,
    pub colorblind_assist: bool,
    pub fullscreen: bool,
    pub gamepad_deadzone: f32,
    pub mouse_sensitivity: f32,
    pub larger_hud_text: bool,
    pub high_contrast_ui: bool,
    pub graphics_quality: GraphicsQualitySetting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphicsQualitySetting {
    Low,
    Medium,
    High,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            master_volume: 0.8,
            music_volume: 0.65,
            sound_effect_volume: 0.85,
            screen_shake: 0.75,
            reduced_motion: false,
            colorblind_assist: true,
            fullscreen: false,
            gamepad_deadzone: 0.18,
            mouse_sensitivity: 1.0,
            larger_hud_text: false,
            high_contrast_ui: false,
            graphics_quality: GraphicsQualitySetting::Medium,
        }
    }
}

impl UserSettings {
    pub fn normalize(&mut self) {
        self.schema_version = PROFILE_SCHEMA_VERSION;
        self.master_volume = self.master_volume.clamp(0.0, 1.0);
        self.music_volume = self.music_volume.clamp(0.0, 1.0);
        self.sound_effect_volume = self.sound_effect_volume.clamp(0.0, 1.0);
        self.screen_shake = self.screen_shake.clamp(0.0, 1.0);
        self.gamepad_deadzone = self.gamepad_deadzone.clamp(0.05, 0.50);
        self.mouse_sensitivity = self.mouse_sensitivity.clamp(0.50, 2.0);
    }
}

/// Non-blocking persistence status shown in Settings when browser storage fails.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct PersistenceStatus {
    pub warning: Option<String>,
    pub loaded: bool,
    pub dirty: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchStatistics {
    pub kills: u32,
    pub deaths: u32,
    pub captures_completed: u32,
    pub area_captured_total: f32,
    pub area_stolen_total: f32,
    pub largest_capture_area: f32,
    pub peak_territory_area: f32,
    pub longest_trail_length: f32,
    pub time_alive_seconds: f32,
}

/// Applies one completed human match to a persistent profile.
pub fn apply_match_statistics(
    lifetime: &mut LifetimeStatistics,
    stats: &MatchStatistics,
    won: bool,
    arena_area: f32,
) {
    lifetime.games_played = lifetime.games_played.saturating_add(1);
    lifetime.wins = lifetime.wins.saturating_add(u32::from(won));
    lifetime.kills = lifetime.kills.saturating_add(stats.kills);
    lifetime.deaths = lifetime.deaths.saturating_add(stats.deaths);
    lifetime.total_captured_area += stats.area_captured_total.max(0.0);
    if arena_area > 0.0 {
        let percent = |area: f32| area.max(0.0) * 100.0 / arena_area;
        lifetime.best_territory_percent = lifetime
            .best_territory_percent
            .max(percent(stats.peak_territory_area));
        lifetime.largest_capture_percent = lifetime
            .largest_capture_percent
            .max(percent(stats.largest_capture_area));
    }
    lifetime.longest_trail_world_units = lifetime
        .longest_trail_world_units
        .max(stats.longest_trail_length);
}

pub fn sanitize_profile_name(requested: &str, fallback: &str) -> String {
    let sanitized: String = requested
        .chars()
        .filter(|ch| !ch.is_control())
        .take(MAX_PROFILE_NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_owned();
    if sanitized.is_empty() {
        fallback.to_owned()
    } else {
        sanitized
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

#[cfg(target_arch = "wasm32")]
fn unix_time_ms() -> u64 {
    js_sys::Date::now().max(0.0) as u64
}

pub struct ProfilesPlugin;

impl Plugin for ProfilesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProfileStore>()
            .init_resource::<UserSettings>()
            .init_resource::<PersistenceStatus>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_names_are_safe_and_character_limited() {
        assert_eq!(
            sanitize_profile_name("  Ada\nLovelace  ", "Player"),
            "AdaLovelace"
        );
        assert_eq!(sanitize_profile_name("", "Player 2"), "Player 2");
        assert_eq!(
            sanitize_profile_name("12345678901234567", "x")
                .chars()
                .count(),
            16
        );
    }

    #[test]
    fn leaderboard_uses_spec_tie_break_order() {
        let mut store = ProfileStore::default();
        store.create("Kills");
        store.create("Territory");
        store.profiles[0].statistics.wins = 2;
        store.profiles[0].statistics.kills = 99;
        store.profiles[1].statistics.wins = 2;
        store.profiles[1].statistics.best_territory_percent = 50.0;
        assert_eq!(store.sorted_leaderboard()[0].display_name, "Territory");
    }

    #[test]
    fn new_profile_stores_are_explicitly_versioned() {
        assert_eq!(
            ProfileStore::default().schema_version,
            PROFILE_SCHEMA_VERSION
        );
    }

    #[test]
    fn temporary_profiles_have_non_empty_stable_ids() {
        let profile = LocalProfile::temporary(2);
        assert!(!profile.id.is_empty());
        assert!(profile.id.ends_with("-2"));
        assert_eq!(profile.created_at_unix_ms, profile.last_used_at_unix_ms);
    }

    #[test]
    fn match_statistics_are_saturating_and_percent_based() {
        let mut lifetime = LifetimeStatistics::default();
        let stats = MatchStatistics {
            kills: 3,
            peak_territory_area: 25.0,
            largest_capture_area: 10.0,
            ..default()
        };
        apply_match_statistics(&mut lifetime, &stats, true, 100.0);
        assert_eq!(lifetime.games_played, 1);
        assert_eq!(lifetime.wins, 1);
        assert_eq!(lifetime.best_territory_percent, 25.0);
        assert_eq!(lifetime.largest_capture_percent, 10.0);
    }

    #[test]
    fn missing_exact_area_fields_default_without_inventing_measurements() {
        let migrated: LifetimeStatistics = serde_json::from_str(
            r#"{
                "best_territory_percent": 8.0
            }"#,
        )
        .unwrap();
        assert_eq!(migrated.total_captured_area, 0.0);
        assert_eq!(migrated.best_territory_percent, 8.0);
        assert_eq!(migrated.games_played, 0);
    }

    #[test]
    fn lifetime_statistics_exact_fields_round_trip() {
        let original = LifetimeStatistics {
            games_played: 4,
            wins: 2,
            kills: 7,
            deaths: 3,
            total_captured_area: 42.25,
            best_territory_percent: 61.5,
            largest_capture_percent: 19.75,
            longest_trail_world_units: 108.0,
        };
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: LifetimeStatistics = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, original);
    }
}
