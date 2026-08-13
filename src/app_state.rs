//! Top-level application flow and shell composition.

use bevy::prelude::*;

use crate::{
    input::InputPlugin, lobby::LobbyPlugin, profiles::ProfilesPlugin, ui::UiPlugin, web::WebPlugin,
};

/// The complete, explicit application state machine.
#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Boot,
    Home,
    Lobby,
    MatchLoading,
    Countdown,
    Playing,
    Paused,
    GameOver,
    Results,
    LocalLeaderboard,
    Settings,
}

impl AppState {
    /// Shell states that keep the attract match running behind their UI.
    pub const fn shows_attract_match(self) -> bool {
        matches!(
            self,
            Self::Home | Self::Lobby | Self::LocalLeaderboard | Self::Settings
        )
    }
}

/// Where Settings should return when dismissed.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsReturn(pub AppState);

impl Default for SettingsReturn {
    fn default() -> Self {
        Self(AppState::Home)
    }
}

/// The application shell is deliberately independent of match simulation.
///
/// Simulation and presentation plugins can consume [`crate::lobby::MatchSetup`]
/// and react to [`AppState::MatchLoading`] without being known by the shell.
pub struct AppShellPlugin;

impl Plugin for AppShellPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .init_resource::<SettingsReturn>()
            .add_plugins((
                ProfilesPlugin,
                InputPlugin,
                LobbyPlugin,
                WebPlugin,
                UiPlugin,
            ))
            .add_systems(OnEnter(AppState::Boot), finish_boot);
    }
}

fn finish_boot(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::Home);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_advances_to_home() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppState>()
            .add_systems(OnEnter(AppState::Boot), finish_boot);
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<State<AppState>>().get(),
            &AppState::Home
        );
    }
}
