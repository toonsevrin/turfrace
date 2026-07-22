//! Responsive, controller-friendly application UI.

mod gameplay_hud;
mod interaction;
mod screens;
mod theme;

use gameplay_hud::*;
use interaction::*;
use screens::*;
use theme::*;

use bevy::{ecs::hierarchy::ChildSpawnerCommands, input::keyboard::KeyboardInput, prelude::*};

use crate::{
    app_state::{AppState, SettingsReturn},
    audio::{AudioCue, PlayAudioCue},
    board::BoardGrid,
    camera::PlayerCamera,
    input::{InputDeviceId, MenuAction, MenuInput},
    lobby::{Lobby, LobbyCommand, LobbyCommandMessage, MatchDisconnectNotice, MatchSetup},
    match_game::{
        Competitor, CompetitorKind, DeathCause, EliminationFeed, LifeState, MatchSession,
        MatchStatistics as SimulationMatchStatistics, Rankings, SpawnProtection, TerritoryRecord,
    },
    palette::palette_color,
    profiles::{
        GraphicsQualitySetting, MatchStatistics as ProfileMatchStatistics, PersistenceStatus,
        ProfileStore, UserSettings, apply_match_statistics, sanitize_profile_name,
    },
    trail::ActiveTrail,
};

// The shell is an ink-and-paper control surface. Competitor colours are used
// as signals, while the neutral UI stays quiet enough to keep eight-player
// information legible.
const INK: Color = Color::srgb(0.035, 0.055, 0.09);
const PAPER: Color = Color::srgb_u8(244, 240, 230);
const LIME: Color = Color::srgb(0.25, 0.55, 0.06);
const CORAL: Color = Color::srgb(1.0, 0.39, 0.30);
const MUTED: Color = Color::srgb(0.24, 0.31, 0.40);
const CREAM: Color = Color::srgb(0.96, 0.93, 0.85);

#[derive(Resource, Clone)]
struct UiTheme {
    display_font: FontSource,
    body_font: FontSource,
}

#[derive(Component)]
struct UiCamera;

#[derive(Component)]
struct ScreenRoot;

#[derive(Component)]
struct DecorativeTrail {
    phase: f32,
    speed: f32,
    amplitude: f32,
}

#[derive(Component, Clone, Debug)]
enum UiAction {
    State(AppState),
    Settings,
    SettingsBack,
    Resume,
    ReplaceDisconnected,
    Lobby(LobbyCommand),
    AdjustSetting(SettingField, f32),
    ToggleSetting(SettingField),
    ResetStatistics,
    ResetAllData,
    CreateProfile(usize),
    RenameProfile(String),
    DeleteProfile(String),
    EditorCharacter(char),
    EditorBackspace,
    EditorSave,
    EditorCancel,
    Rematch { same_field: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingField {
    Master,
    Music,
    SoundEffects,
    ScreenShake,
    ReducedMotion,
    Colorblind,
    Fullscreen,
    GamepadDeadzone,
    MouseSensitivity,
    LargerHudText,
    HighContrast,
    GraphicsQuality,
}

#[derive(Component)]
struct FocusOrder(u16);

#[derive(Resource, Default)]
struct UiFocus {
    entity: Option<Entity>,
}

#[derive(Message, Clone)]
struct UiActivated(UiAction);

#[derive(Resource, Default)]
struct LobbyFingerprint(String);

#[derive(Component)]
struct SettingsValueText(SettingField);

#[derive(Component)]
struct NameEditorOverlay;

#[derive(Component)]
struct NameEditorValue;

#[derive(Component)]
struct GameplayHudRoot;

#[derive(Component)]
struct GlobalRankingText;

#[derive(Component)]
struct KillFeedText;

#[derive(Component)]
struct MatchAnnouncementText;

#[derive(Component)]
struct HumanHudRoot {
    camera: Entity,
}

#[derive(Component)]
struct HumanHudSummary(Entity);

#[derive(Component)]
struct HumanHudRank(Entity);

#[derive(Component)]
struct HumanRespawnText(Entity);

#[derive(Resource, Default)]
struct Confirmation {
    action: Option<ConfirmationAction>,
    remaining: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConfirmationAction {
    ResetStatistics,
    ResetAllData,
    DeleteProfile(String),
}

#[derive(Resource, Default)]
struct NameEditor {
    target: Option<NameEditorTarget>,
    buffer: String,
}

#[derive(Debug, Clone)]
enum NameEditorTarget {
    LobbySlot(usize),
    ExistingProfile(String),
}

#[derive(Resource, Default)]
struct PersistedMatchSeed(Option<u64>);

#[derive(Resource, Debug, Clone, Default)]
pub struct MatchResults {
    pub winner_name: String,
    pub duration_seconds: f32,
    pub rows: Vec<ResultRow>,
}

#[derive(Debug, Clone, Default)]
pub struct ResultRow {
    pub name: String,
    pub color_id: u8,
    pub placement: u8,
    pub peak_percent: f32,
    pub kills: u32,
    pub deaths: u32,
    pub largest_capture_percent: f32,
    pub total_cells_captured: u32,
    pub longest_trail: f32,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiFocus>()
            .init_resource::<LobbyFingerprint>()
            .init_resource::<Confirmation>()
            .init_resource::<NameEditor>()
            .init_resource::<MatchResults>()
            .init_resource::<PersistedMatchSeed>()
            .add_message::<UiActivated>()
            .add_systems(Startup, setup_ui)
            .add_systems(OnEnter(AppState::Home), spawn_home)
            .add_systems(OnExit(AppState::Home), cleanup_screen)
            .add_systems(OnEnter(AppState::Lobby), spawn_lobby)
            .add_systems(OnExit(AppState::Lobby), cleanup_screen)
            .add_systems(OnEnter(AppState::MatchLoading), spawn_match_loading)
            .add_systems(OnExit(AppState::MatchLoading), cleanup_screen)
            .add_systems(OnEnter(AppState::LocalLeaderboard), spawn_leaderboard)
            .add_systems(OnExit(AppState::LocalLeaderboard), cleanup_screen)
            .add_systems(OnEnter(AppState::Settings), spawn_settings)
            .add_systems(OnExit(AppState::Settings), cleanup_screen)
            .add_systems(OnEnter(AppState::Paused), spawn_pause)
            .add_systems(OnExit(AppState::Paused), cleanup_screen)
            .add_systems(
                OnEnter(AppState::GameOver),
                (collect_match_results, spawn_game_over).chain(),
            )
            .add_systems(OnExit(AppState::GameOver), cleanup_screen)
            .add_systems(
                OnEnter(AppState::Results),
                (cleanup_gameplay_hud, spawn_results).chain(),
            )
            .add_systems(OnExit(AppState::Results), cleanup_screen)
            .add_systems(OnEnter(AppState::Countdown), spawn_gameplay_hud)
            .add_systems(OnEnter(AppState::Lobby), cleanup_gameplay_hud)
            .add_systems(
                Update,
                (
                    animate_background,
                    button_interactions,
                    controller_navigation,
                    dispatch_ui_actions,
                    update_button_focus,
                    pause_input,
                    scroll_lobby.run_if(in_state(AppState::Lobby)),
                    update_lobby_screen.run_if(in_state(AppState::Lobby)),
                    refresh_settings_labels.run_if(in_state(AppState::Settings)),
                    edit_profile_name,
                    tick_confirmation,
                ),
            )
            .add_systems(Update, (reconcile_human_huds, update_gameplay_hud));
    }
}

// Theme primitives and shell camera setup live in `theme`.
// Screen builders live in `screens`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_values_are_clamped_by_dispatch_contract() {
        let mut settings = UserSettings {
            master_volume: 1.7,
            screen_shake: -2.0,
            ..default()
        };
        settings.normalize();
        assert_eq!(settings.master_volume, 1.0);
        assert_eq!(settings.screen_shake, 0.0);
    }
}
