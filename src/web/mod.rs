//! Small platform boundary for persistence, focus behavior, and fullscreen.

use bevy::{
    prelude::*,
    window::{PrimaryWindow, WindowFocused},
};

#[cfg(not(target_arch = "wasm32"))]
use bevy::window::{MonitorSelection, WindowMode};

use crate::{
    app_state::AppState,
    lobby::LastLobbySettings,
    profiles::{PersistenceStatus, ProfileStore, UserSettings},
};

#[cfg(target_arch = "wasm32")]
use crate::profiles::GraphicsQualitySetting;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function turfrace_set_fullscreen(enabled) {
    const shell = document.getElementById("game-shell");
    if (enabled && !document.fullscreenElement) {
        shell?.requestFullscreen?.().catch(() => {});
    } else if (!enabled && document.fullscreenElement) {
        document.exitFullscreen?.().catch(() => {});
    }
}
"#)]
extern "C" {
    fn turfrace_set_fullscreen(enabled: bool);
}

pub const PROFILES_KEY: &str = "turfrace.profiles.v1";
pub const SETTINGS_KEY: &str = "turfrace.settings.v1";
pub const LAST_LOBBY_KEY: &str = "turfrace.last_lobby.v1";

pub struct WebPlugin;

impl Plugin for WebPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_local_data).add_systems(
            Update,
            (
                save_changed_local_data,
                pause_when_hidden,
                pause_when_window_unfocused,
                apply_fullscreen,
            ),
        );
    }
}

fn load_local_data(
    profiles: ResMut<ProfileStore>,
    mut settings: ResMut<UserSettings>,
    last_lobby: ResMut<LastLobbySettings>,
    mut status: ResMut<PersistenceStatus>,
) {
    #[cfg(target_arch = "wasm32")]
    {
        let mut profiles = profiles;
        let mut last_lobby = last_lobby;
        match browser_storage() {
            Ok(storage) => {
                load_value(&storage, PROFILES_KEY, &mut *profiles, &mut status);
                load_value(&storage, SETTINGS_KEY, &mut *settings, &mut status);
                load_value(&storage, LAST_LOBBY_KEY, &mut *last_lobby, &mut status);
            }
            Err(error) => status.warning = Some(error),
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (profiles, last_lobby);
    settings.normalize();
    status.loaded = true;
    status.dirty = false;
}

fn save_changed_local_data(
    profiles: Res<ProfileStore>,
    settings: Res<UserSettings>,
    last_lobby: Res<LastLobbySettings>,
    mut status: ResMut<PersistenceStatus>,
) {
    if !status.loaded
        || !(status.dirty
            || profiles.is_changed()
            || settings.is_changed()
            || last_lobby.is_changed())
    {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    {
        let result = browser_storage().and_then(|storage| {
            save_value(&storage, PROFILES_KEY, &*profiles)?;
            save_value(&storage, SETTINGS_KEY, &*settings)?;
            save_value(&storage, LAST_LOBBY_KEY, &*last_lobby)
        });
        if let Err(error) = result {
            status.warning = Some(error);
        }
    }
    status.dirty = false;
}

#[cfg(target_arch = "wasm32")]
fn browser_storage() -> Result<web_sys::Storage, String> {
    web_sys::window()
        .ok_or_else(|| {
            "Browser storage is unavailable; progress will last for this session.".to_owned()
        })?
        .local_storage()
        .map_err(|_| {
            "Browser storage permission was denied; progress will last for this session.".to_owned()
        })?
        .ok_or_else(|| {
            "Browser storage is disabled; progress will last for this session.".to_owned()
        })
}

#[cfg(target_arch = "wasm32")]
fn load_value<T: serde::de::DeserializeOwned>(
    storage: &web_sys::Storage,
    key: &str,
    destination: &mut T,
    status: &mut PersistenceStatus,
) {
    let loaded = storage
        .get_item(key)
        .map_err(|_| "Could not read browser storage.".to_owned())
        .and_then(|value| match value {
            Some(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|_| "Some saved data was invalid and has been safely ignored.".to_owned()),
            None => Ok(None),
        });
    match loaded {
        Ok(Some(value)) => *destination = value,
        Ok(None) => {}
        Err(error) => status.warning = Some(error),
    }
}

#[cfg(target_arch = "wasm32")]
fn save_value<T: serde::Serialize>(
    storage: &web_sys::Storage,
    key: &str,
    value: &T,
) -> Result<(), String> {
    let json =
        serde_json::to_string(value).map_err(|_| "Could not encode local data.".to_owned())?;
    storage.set_item(key, &json).map_err(|_| {
        "Browser storage is full or unavailable; recent progress may not persist.".to_owned()
    })
}

fn pause_when_hidden(state: Res<State<AppState>>, mut next: ResMut<NextState<AppState>>) {
    if !should_auto_pause(*state.get(), true, page_is_hidden()) {
        return;
    }
    next.set(AppState::Paused);
}

fn pause_when_window_unfocused(
    mut focused: MessageReader<WindowFocused>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if focused
        .read()
        .any(|event| should_auto_pause(*state.get(), event.focused, false))
    {
        next.set(AppState::Paused);
    }
}

fn should_auto_pause(state: AppState, focused: bool, hidden: bool) -> bool {
    state == AppState::Playing && (!focused || hidden)
}

#[cfg(target_arch = "wasm32")]
fn page_is_hidden() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .is_some_and(|document| document.hidden())
}

#[cfg(not(target_arch = "wasm32"))]
fn page_is_hidden() -> bool {
    false
}

fn apply_fullscreen(
    settings: Res<UserSettings>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !settings.is_changed() {
        return;
    }
    let Ok(mut window) = window.single_mut() else {
        return;
    };
    #[cfg(target_arch = "wasm32")]
    {
        window
            .resolution
            .set_scale_factor_override(Some(match settings.graphics_quality {
                GraphicsQualitySetting::Low => 1.0,
                GraphicsQualitySetting::Medium => 1.5,
                GraphicsQualitySetting::High => 2.0,
            }));
        turfrace_set_fullscreen(settings.fullscreen);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        window.mode = if settings.fullscreen {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistence_keys_are_versioned_and_distinct() {
        let keys = [PROFILES_KEY, SETTINGS_KEY, LAST_LOBBY_KEY];
        assert!(keys.iter().all(|key| key.ends_with(".v1")));
        assert_ne!(keys[0], keys[1]);
        assert_ne!(keys[1], keys[2]);
    }

    #[test]
    fn only_an_active_match_auto_pauses_for_focus_or_visibility() {
        assert!(should_auto_pause(AppState::Playing, false, false));
        assert!(should_auto_pause(AppState::Playing, true, true));
        assert!(!should_auto_pause(AppState::Home, false, true));
        assert!(!should_auto_pause(AppState::Playing, true, false));
    }
}
