//! Human device registration and match composition.

use std::collections::HashSet;

use bevy::{ecs::system::SystemParam, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{
    app_state::AppState,
    audio::{AudioCue, PlayAudioCue},
    input::{DeviceDisconnected, HumanController, InputDeviceId, MenuAction, MenuInput},
    match_game::{Competitor, CompetitorKind},
    npc::{NpcController, NpcDifficulty, deterministic_personality},
    profiles::{LocalProfile, PersistenceStatus, ProfileStore},
};

pub const MAX_HUMANS: usize = 8;
pub const MIN_COMPETITORS: u8 = 2;
pub const MAX_COMPETITORS: u8 = 12;
pub const PLAYER_COLOR_COUNT: u8 = crate::palette::PLAYER_COLORS.len() as u8;

#[derive(Debug, Clone, PartialEq)]
pub struct LobbyPlayer {
    pub device: InputDeviceId,
    pub profile_id: Option<String>,
    pub display_name: String,
    pub color_id: u8,
    pub pattern_id: u8,
    pub ready: bool,
    pub connected: bool,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
    /// Robots selected independently of the joined human count.
    pub npc_count: u8,
    pub shared_focus_owner: Option<InputDeviceId>,
}

impl Lobby {
    pub fn npc_count(&self) -> u8 {
        self.npc_count
    }

    pub fn total_competitors(&self) -> u8 {
        self.players.len() as u8 + self.npc_count
    }

    pub fn max_npc_count(&self) -> u8 {
        MAX_COMPETITORS.saturating_sub(self.players.len() as u8)
    }

    pub fn can_start(&self) -> bool {
        self.players.len() >= usize::from(MIN_COMPETITORS)
            && self
                .players
                .iter()
                .all(|player| player.ready && player.connected)
    }

    pub fn set_npc_count(&mut self, requested: i16) {
        self.npc_count = requested.clamp(0, i16::from(self.max_npc_count())) as u8;
    }

    pub fn slot_for_device(&self, device: InputDeviceId) -> Option<usize> {
        self.players
            .iter()
            .position(|player| player.device == device)
    }

    pub fn join(&mut self, device: InputDeviceId, profiles: &ProfileStore) -> bool {
        if self.players.len() >= MAX_HUMANS || self.slot_for_device(device).is_some() {
            return false;
        }
        let used_profiles: HashSet<&str> = self
            .players
            .iter()
            .filter_map(|player| player.profile_id.as_deref())
            .collect();
        let profile = profiles
            .profiles
            .iter()
            .filter(|profile| !used_profiles.contains(profile.id.as_str()))
            .max_by_key(|profile| profile.last_used_at_unix_ms);
        let number = self.players.len() + 1;
        let fallback = LocalProfile::temporary(number);
        let selected = profile.unwrap_or(&fallback);
        let occupied_colors: HashSet<u8> =
            self.players.iter().map(|player| player.color_id).collect();
        let color_id = first_available_color(selected.preferred_color_id, &occupied_colors);
        self.players.push(LobbyPlayer {
            device,
            profile_id: profile.map(|profile| profile.id.clone()),
            display_name: selected.display_name.clone(),
            color_id,
            pattern_id: selected.icon_id % 8,
            ready: false,
            connected: true,
        });
        self.npc_count = self.npc_count.min(self.max_npc_count());
        self.shared_focus_owner = Some(device);
        true
    }

    pub fn leave(&mut self, device: InputDeviceId) -> bool {
        let before = self.players.len();
        self.players.retain(|player| player.device != device);
        before != self.players.len()
    }

    pub fn cycle_color(&mut self, slot: usize, direction: i8) {
        if slot >= self.players.len() {
            return;
        }
        let occupied: HashSet<u8> = self
            .players
            .iter()
            .enumerate()
            .filter_map(|(index, player)| (index != slot).then_some(player.color_id))
            .collect();
        let mut candidate = self.players[slot].color_id;
        for _ in 0..PLAYER_COLOR_COUNT {
            candidate = (i16::from(candidate) + i16::from(direction))
                .rem_euclid(i16::from(PLAYER_COLOR_COUNT)) as u8;
            if !occupied.contains(&candidate) {
                self.players[slot].color_id = candidate;
                break;
            }
        }
    }

    pub fn cycle_profile(&mut self, slot: usize, direction: i8, profiles: &ProfileStore) {
        if slot >= self.players.len() || profiles.profiles.is_empty() {
            return;
        }
        let occupied: HashSet<&str> = self
            .players
            .iter()
            .enumerate()
            .filter_map(|(index, player)| {
                (index != slot)
                    .then_some(player.profile_id.as_deref())
                    .flatten()
            })
            .collect();
        let current = self.players[slot]
            .profile_id
            .as_deref()
            .and_then(|id| {
                profiles
                    .profiles
                    .iter()
                    .position(|profile| profile.id == id)
            })
            .unwrap_or(0);
        for distance in 1..=profiles.profiles.len() {
            let index = (current as isize + isize::from(direction) * distance as isize)
                .rem_euclid(profiles.profiles.len() as isize) as usize;
            let profile = &profiles.profiles[index];
            if !occupied.contains(profile.id.as_str()) {
                self.players[slot].profile_id = Some(profile.id.clone());
                self.players[slot].display_name = profile.display_name.clone();
                break;
            }
        }
    }
}

fn first_available_color(preferred: u8, occupied: &HashSet<u8>) -> u8 {
    (0..PLAYER_COLOR_COUNT)
        .map(|offset| (preferred + offset) % PLAYER_COLOR_COUNT)
        .find(|color| !occupied.contains(color))
        .unwrap_or(preferred % PLAYER_COLOR_COUNT)
}

#[derive(Debug, Clone, PartialEq)]
pub struct HumanSetup {
    pub device: InputDeviceId,
    pub profile_id: Option<String>,
    pub display_name: String,
    pub color_id: u8,
    pub pattern_id: u8,
}

#[derive(Resource, Debug, Clone, PartialEq)]
pub struct MatchSetup {
    pub seed: u64,
    pub total_competitors: u8,
    pub humans: Vec<HumanSetup>,
    pub replay_same_field: bool,
}

impl MatchSetup {
    /// Converts a disconnected human slot into an NPC without changing the
    /// stable competitor ordering of the remaining human slots.
    pub fn replace_human_with_npc(&mut self, device: InputDeviceId) -> bool {
        let before = self.humans.len();
        self.humans.retain(|human| human.device != device);
        before != self.humans.len()
    }
}

#[derive(Resource, Debug, Clone, PartialEq)]
pub struct MatchDisconnectNotice {
    pub device: Option<InputDeviceId>,
    pub player_name: String,
    pub replace_with_npc: bool,
    pub resume_state: AppState,
}

impl Default for MatchDisconnectNotice {
    fn default() -> Self {
        Self {
            device: None,
            player_name: String::new(),
            replace_with_npc: false,
            resume_state: AppState::Playing,
        }
    }
}

impl Default for MatchSetup {
    fn default() -> Self {
        Self {
            seed: 1,
            total_competitors: MIN_COMPETITORS,
            humans: Vec::new(),
            replay_same_field: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LobbyCommand {
    Join(InputDeviceId),
    Leave(InputDeviceId),
    ToggleReady(InputDeviceId),
    ChangeNpcCount(i8),
    CycleColor(InputDeviceId, i8),
    CycleProfile(InputDeviceId, i8),
    CyclePattern(InputDeviceId, i8),
    Start,
}

#[derive(Message, Debug, Clone, Copy)]
pub struct LobbyCommandMessage(pub LobbyCommand);

#[derive(Resource, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LastLobbySettings {
    pub schema_version: u32,
    pub npc_count: u8,
}

impl Default for LastLobbySettings {
    fn default() -> Self {
        Self {
            schema_version: 2,
            npc_count: 0,
        }
    }
}

impl<'de> Deserialize<'de> for LastLobbySettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct StoredSettings {
            #[serde(default)]
            schema_version: u32,
            #[serde(default)]
            npc_count: Option<u8>,
            #[serde(default)]
            total_competitors: Option<u8>,
        }

        let stored = StoredSettings::deserialize(deserializer)?;
        let npc_count = stored.npc_count.unwrap_or_else(|| {
            // Version 1 stored a target field size. It assumed the normal
            // two-human lobby, so retain the equivalent robot preference.
            stored
                .total_competitors
                .unwrap_or(MIN_COMPETITORS)
                .saturating_sub(MIN_COMPETITORS)
        });
        let _ = stored.schema_version;
        Ok(Self {
            schema_version: 2,
            npc_count: npc_count.min(MAX_COMPETITORS - 1),
        })
    }
}

pub struct LobbyPlugin;

#[derive(SystemParam)]
struct LobbyLaunch<'w> {
    setup: ResMut<'w, MatchSetup>,
    next: ResMut<'w, NextState<AppState>>,
}

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Lobby>()
            .init_resource::<MatchSetup>()
            .init_resource::<MatchDisconnectNotice>()
            .init_resource::<LastLobbySettings>()
            .add_message::<LobbyCommandMessage>()
            .add_systems(OnEnter(AppState::Lobby), restore_lobby_settings)
            .add_systems(
                Update,
                (
                    translate_menu_input,
                    handle_disconnections,
                    apply_lobby_commands,
                )
                    .chain()
                    .run_if(in_state(AppState::Lobby)),
            )
            .add_systems(
                Update,
                (
                    pause_for_match_disconnect,
                    reclaim_disconnected_slot,
                    replace_disconnected_with_npc,
                ),
            );
    }
}

fn pause_for_match_disconnect(
    mut disconnected: MessageReader<DeviceDisconnected>,
    state: Res<State<AppState>>,
    setup: Res<MatchSetup>,
    mut notice: ResMut<MatchDisconnectNotice>,
    mut next: ResMut<NextState<AppState>>,
) {
    if !matches!(*state.get(), AppState::Playing | AppState::Countdown) {
        return;
    }
    for event in disconnected.read() {
        if let Some(human) = setup.humans.iter().find(|human| human.device == event.0) {
            notice.device = Some(event.0);
            notice.player_name = human.display_name.clone();
            notice.replace_with_npc = false;
            notice.resume_state = *state.get();
            next.set(AppState::Paused);
        }
    }
}

fn reclaim_disconnected_slot(
    mut inputs: MessageReader<MenuInput>,
    state: Res<State<AppState>>,
    mut setup: ResMut<MatchSetup>,
    mut lobby: ResMut<Lobby>,
    mut notice: ResMut<MatchDisconnectNotice>,
    mut next: ResMut<NextState<AppState>>,
    mut controllers: Query<&mut HumanController>,
) {
    if *state.get() != AppState::Paused {
        return;
    }
    let Some(disconnected) = notice.device else {
        return;
    };
    for input in inputs.read() {
        if !matches!(input.action, MenuAction::Confirm | MenuAction::Pause)
            || setup
                .humans
                .iter()
                .any(|human| human.device == input.device)
        {
            continue;
        }
        if let Some(human) = setup
            .humans
            .iter_mut()
            .find(|human| human.device == disconnected)
        {
            human.device = input.device;
        }
        for mut controller in &mut controllers {
            if controller.device == disconnected {
                controller.device = input.device;
            }
        }
        if let Some(player) = lobby
            .players
            .iter_mut()
            .find(|player| player.device == disconnected)
        {
            player.device = input.device;
            player.connected = true;
        }
        notice.device = None;
        notice.player_name.clear();
        notice.replace_with_npc = false;
        next.set(notice.resume_state);
        break;
    }
}

fn replace_disconnected_with_npc(
    mut commands: Commands,
    mut notice: ResMut<MatchDisconnectNotice>,
    mut setup: ResMut<MatchSetup>,
    mut humans: Query<(Entity, &HumanController, &mut Competitor)>,
    mut next: ResMut<NextState<AppState>>,
) {
    if !notice.replace_with_npc {
        return;
    }
    let Some(device) = notice.device else { return };
    for (entity, controller, mut competitor) in &mut humans {
        if controller.device != device {
            continue;
        }
        competitor.kind = CompetitorKind::Npc;
        let id = competitor.id;
        commands
            .entity(entity)
            .remove::<HumanController>()
            .insert(NpcController::standard(
                id,
                deterministic_personality(setup.seed, id),
                NpcDifficulty::Normal,
            ));
    }
    setup.replace_human_with_npc(device);
    notice.device = None;
    notice.player_name.clear();
    notice.replace_with_npc = false;
    next.set(notice.resume_state);
}

fn restore_lobby_settings(last: Res<LastLobbySettings>, mut lobby: ResMut<Lobby>) {
    lobby.set_npc_count(i16::from(last.npc_count));
}

fn translate_menu_input(
    mut input: MessageReader<MenuInput>,
    lobby: Res<Lobby>,
    mut commands: MessageWriter<LobbyCommandMessage>,
) {
    for input in input.read() {
        let command = lobby_command_for_input(&lobby, *input);
        if let Some(command) = command {
            commands.write(LobbyCommandMessage(command));
        }
    }
}

fn lobby_command_for_input(lobby: &Lobby, input: MenuInput) -> Option<LobbyCommand> {
    let slot = lobby.slot_for_device(input.device);
    match (slot, input.action) {
        (
            None,
            MenuAction::Join
            | MenuAction::Confirm
            | MenuAction::Secondary
            | MenuAction::Pause
            | MenuAction::Up
            | MenuAction::Down
            | MenuAction::Left
            | MenuAction::Right
            | MenuAction::ColorPrevious
            | MenuAction::ColorNext,
        ) => Some(LobbyCommand::Join(input.device)),
        (Some(_), MenuAction::Join | MenuAction::Confirm) => {
            Some(LobbyCommand::ToggleReady(input.device))
        }
        (Some(_), MenuAction::Back) => Some(LobbyCommand::Leave(input.device)),
        (Some(_), MenuAction::Pause) => Some(LobbyCommand::Start),
        (Some(_), MenuAction::ColorPrevious) => Some(LobbyCommand::CycleColor(input.device, -1)),
        (Some(_), MenuAction::ColorNext) => Some(LobbyCommand::CycleColor(input.device, 1)),
        _ => None,
    }
}

fn handle_disconnections(
    mut disconnected: MessageReader<DeviceDisconnected>,
    mut lobby: ResMut<Lobby>,
) {
    for event in disconnected.read() {
        if let Some(slot) = lobby.slot_for_device(event.0) {
            lobby.players[slot].connected = false;
            lobby.players[slot].ready = false;
        }
    }
}

fn apply_lobby_commands(
    mut messages: MessageReader<LobbyCommandMessage>,
    mut lobby: ResMut<Lobby>,
    profiles: Res<ProfileStore>,
    mut last: ResMut<LastLobbySettings>,
    mut persistence: ResMut<PersistenceStatus>,
    mut audio: MessageWriter<PlayAudioCue>,
    mut launch: LobbyLaunch,
) {
    for message in messages.read() {
        match message.0 {
            LobbyCommand::Join(device) => {
                if lobby.join(device, &profiles) {
                    audio.write(PlayAudioCue::human(AudioCue::PlayerJoin));
                }
            }
            LobbyCommand::Leave(device) => {
                lobby.leave(device);
            }
            LobbyCommand::ToggleReady(device) => {
                if let Some(slot) = lobby.slot_for_device(device) {
                    lobby.players[slot].ready = !lobby.players[slot].ready;
                    audio.write(PlayAudioCue::human(AudioCue::Ready));
                }
            }
            LobbyCommand::ChangeNpcCount(delta) => {
                let requested = i16::from(lobby.npc_count()) + i16::from(delta);
                lobby.set_npc_count(requested);
                last.npc_count = lobby.npc_count();
                persistence.dirty = true;
            }
            LobbyCommand::CycleColor(device, direction) => {
                if let Some(slot) = lobby.slot_for_device(device) {
                    lobby.cycle_color(slot, direction);
                }
            }
            LobbyCommand::CycleProfile(device, direction) => {
                if let Some(slot) = lobby.slot_for_device(device) {
                    lobby.cycle_profile(slot, direction, &profiles);
                }
            }
            LobbyCommand::CyclePattern(device, direction) => {
                if let Some(slot) = lobby.slot_for_device(device) {
                    lobby.players[slot].pattern_id = (i16::from(lobby.players[slot].pattern_id)
                        + i16::from(direction))
                    .rem_euclid(8) as u8;
                }
            }
            LobbyCommand::Start if lobby.can_start() => {
                prepare_match_setup(&lobby, &mut launch.setup);
                audio.write(PlayAudioCue::human(AudioCue::MenuConfirm));
                launch.next.set(AppState::MatchLoading);
            }
            LobbyCommand::Start => {}
        }
    }
}

fn prepare_match_setup(lobby: &Lobby, setup: &mut MatchSetup) {
    let previous_seed = setup.seed;
    *setup = MatchSetup {
        seed: previous_seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1),
        total_competitors: lobby.total_competitors(),
        humans: lobby
            .players
            .iter()
            .map(|player| HumanSetup {
                device: player.device,
                profile_id: player.profile_id.clone(),
                display_name: player.display_name.clone(),
                color_id: player.color_id,
                pattern_id: player.pattern_id,
            })
            .collect(),
        replay_same_field: false,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robot_count_is_explicit_and_clamped_to_available_slots() {
        let profiles = ProfileStore::default();
        let mut lobby = Lobby::default();
        assert_eq!(lobby.npc_count(), 0);
        for index in 0..5 {
            lobby.join(InputDeviceId::Gamepad(index), &profiles);
        }
        lobby.set_npc_count(99);
        assert_eq!(lobby.npc_count(), 7);
        assert_eq!(lobby.total_competitors(), 12);
    }

    #[test]
    fn start_requires_two_connected_ready_humans() {
        let profiles = ProfileStore::default();
        let mut lobby = Lobby::default();
        lobby.join(InputDeviceId::Mouse, &profiles);
        assert!(!lobby.can_start());
        lobby.players[0].ready = true;
        assert!(!lobby.can_start());
        lobby.set_npc_count(1);
        assert!(!lobby.can_start());
        lobby.join(InputDeviceId::KeyboardPrimary, &profiles);
        lobby.players[1].ready = true;
        assert!(lobby.can_start());
        lobby.players[1].connected = false;
        assert!(!lobby.can_start());
    }

    #[test]
    fn preparing_a_match_commits_players_without_a_second_lobby_countdown() {
        let profiles = ProfileStore::default();
        let mut lobby = Lobby::default();
        lobby.join(InputDeviceId::Mouse, &profiles);
        lobby.players[0].ready = true;
        let mut setup = MatchSetup::default();

        prepare_match_setup(&lobby, &mut setup);

        assert_eq!(setup.humans.len(), 1);
        assert_eq!(setup.humans[0].device, InputDeviceId::Mouse);
        assert_eq!(setup.total_competitors, lobby.total_competitors());
    }

    #[test]
    fn version_one_lobby_preferences_migrate_to_robot_count() {
        let migrated: LastLobbySettings =
            serde_json::from_str(r#"{"schema_version":1,"total_competitors":8}"#).unwrap();
        assert_eq!(migrated.schema_version, 2);
        assert_eq!(migrated.npc_count, 6);
    }

    #[test]
    fn version_two_lobby_preferences_round_trip() {
        let settings = LastLobbySettings {
            schema_version: 2,
            npc_count: 4,
        };
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<LastLobbySettings>(&encoded).unwrap(),
            settings
        );
    }

    #[test]
    fn human_colors_remain_unique_when_cycled() {
        let profiles = ProfileStore::default();
        let mut lobby = Lobby::default();
        lobby.join(InputDeviceId::Mouse, &profiles);
        lobby.join(InputDeviceId::Gamepad(1), &profiles);
        lobby.cycle_color(1, -1);
        assert_ne!(lobby.players[0].color_id, lobby.players[1].color_id);
    }

    #[test]
    fn an_assigned_mouse_can_toggle_ready_with_join_action() {
        let profiles = ProfileStore::default();
        let mut lobby = Lobby::default();
        let device = InputDeviceId::Mouse;
        assert_eq!(
            lobby_command_for_input(
                &lobby,
                MenuInput {
                    device,
                    action: MenuAction::Join,
                },
            ),
            Some(LobbyCommand::Join(device))
        );
        lobby.join(device, &profiles);
        assert_eq!(
            lobby_command_for_input(
                &lobby,
                MenuInput {
                    device,
                    action: MenuAction::Join,
                },
            ),
            Some(LobbyCommand::ToggleReady(device))
        );
    }

    #[test]
    fn unassigned_keyboard_movement_joins_without_consuming_back() {
        let lobby = Lobby::default();
        assert_eq!(
            lobby_command_for_input(
                &lobby,
                MenuInput {
                    device: InputDeviceId::KeyboardPrimary,
                    action: MenuAction::Up,
                },
            ),
            Some(LobbyCommand::Join(InputDeviceId::KeyboardPrimary))
        );
        assert_eq!(
            lobby_command_for_input(
                &lobby,
                MenuInput {
                    device: InputDeviceId::KeyboardPrimary,
                    action: MenuAction::Back,
                },
            ),
            None
        );
    }

    #[test]
    fn keyboard_and_mouse_can_join_separate_slots() {
        let profiles = ProfileStore::default();
        let mut lobby = Lobby::default();
        assert!(lobby.join(InputDeviceId::KeyboardPrimary, &profiles));
        assert!(lobby.join(InputDeviceId::Mouse, &profiles));
        assert_eq!(
            lobby.slot_for_device(InputDeviceId::KeyboardPrimary),
            Some(0)
        );
        assert_eq!(lobby.slot_for_device(InputDeviceId::Mouse), Some(1));
    }

    #[test]
    fn unassigned_controller_navigation_joins() {
        let lobby = Lobby::default();
        let device = InputDeviceId::Gamepad(3);
        assert_eq!(
            lobby_command_for_input(
                &lobby,
                MenuInput {
                    device,
                    action: MenuAction::Right,
                },
            ),
            Some(LobbyCommand::Join(device))
        );
    }
}
