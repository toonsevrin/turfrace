//! Human device registration and match composition.

use std::collections::HashSet;

use bevy::{ecs::system::SystemParam, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{
    app_state::AppState,
    audio::{AudioCue, PlayAudioCue},
    input::{DeviceDisconnected, HumanController, InputDeviceId, MenuAction, MenuInput},
    match_game::{Competitor, CompetitorKind},
    npc::{NpcController, NpcDifficulty, generate_npc_roster},
    profiles::{LocalProfile, PersistenceStatus, ProfileStore},
};

pub const MAX_HUMANS: usize = 8;
pub const MIN_COMPETITORS: u8 = 2;
pub const MAX_COMPETITORS: u8 = 12;
pub const PLAYER_COLOR_COUNT: u8 = crate::palette::PLAYER_COLORS.len() as u8;
pub const READY_COUNTDOWN_SECONDS: f32 = 3.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LobbyProfileSelection {
    Temporary,
    Saved(String),
    CreateNew,
}

impl LobbyProfileSelection {
    pub fn saved_id(&self) -> Option<&str> {
        match self {
            Self::Saved(id) => Some(id),
            Self::Temporary | Self::CreateNew => None,
        }
    }

    pub fn is_create_new(&self) -> bool {
        matches!(self, Self::CreateNew)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LobbyPlayer {
    pub device: InputDeviceId,
    pub profile: LobbyProfileSelection,
    pub display_name: String,
    pub color_id: u8,
    pub pattern_id: u8,
    pub ready: bool,
    pub connected: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum LobbyLaunchState {
    #[default]
    Waiting,
    CountingDown(f32),
    Launching,
}

impl LobbyLaunchState {
    pub fn remaining(self) -> Option<f32> {
        match self {
            Self::CountingDown(remaining) => Some(remaining),
            Self::Waiting | Self::Launching => None,
        }
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
    /// Robots selected independently of the joined human count.
    pub npc_count: u8,
    pub npc_difficulty: NpcDifficulty,
    pub shared_focus_owner: Option<InputDeviceId>,
    /// The lobby remains interactive during the countdown so any racer can
    /// unready and cancel launch without entering a second confirmation flow.
    pub launch_state: LobbyLaunchState,
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
        // NPCs count toward the minimum field size. This lets one local
        // player start a full-screen race against at least one robot while
        // still preventing a race with no opponent.
        self.total_competitors() >= MIN_COMPETITORS
            && !self.players.is_empty()
            && self
                .players
                .iter()
                .all(|player| player.ready && player.connected && !player.profile.is_create_new())
    }

    pub fn set_npc_count(&mut self, requested: i16) {
        let count = requested.clamp(0, i16::from(self.max_npc_count())) as u8;
        if self.npc_count != count {
            self.npc_count = count;
            self.cancel_launch();
        }
    }

    pub fn cycle_npc_difficulty(&mut self, direction: i8) {
        self.npc_difficulty = self.npc_difficulty.cycle(direction);
        self.cancel_launch();
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
            .filter_map(|player| player.profile.saved_id())
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
        let pattern_id = assigned_pattern(device, number);
        self.cancel_launch();
        self.players.push(LobbyPlayer {
            device,
            profile: profile
                .map(|profile| LobbyProfileSelection::Saved(profile.id.clone()))
                .unwrap_or(LobbyProfileSelection::Temporary),
            display_name: selected.display_name.clone(),
            color_id,
            pattern_id,
            ready: false,
            connected: true,
        });
        self.npc_count = self.npc_count.min(self.max_npc_count());
        self.shared_focus_owner = Some(device);
        true
    }

    pub fn clear_humans(&mut self) {
        self.players.clear();
        self.shared_focus_owner = None;
        self.cancel_launch();
    }

    pub fn leave(&mut self, device: InputDeviceId) -> bool {
        let before = self.players.len();
        self.players.retain(|player| player.device != device);
        let changed = before != self.players.len();
        if changed {
            self.cancel_launch();
        }
        changed
    }

    fn cancel_launch(&mut self) {
        self.launch_state = LobbyLaunchState::Waiting;
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
                self.players[slot].ready = false;
                self.cancel_launch();
                break;
            }
        }
    }

    pub fn cycle_profile(&mut self, slot: usize, direction: i8, profiles: &ProfileStore) {
        if slot >= self.players.len() {
            return;
        }
        let occupied: HashSet<&str> = self
            .players
            .iter()
            .enumerate()
            .filter_map(|(index, player)| {
                (index != slot).then(|| player.profile.saved_id()).flatten()
            })
            .collect();
        let available: Vec<_> = profiles
            .profiles
            .iter()
            .filter(|profile| !occupied.contains(profile.id.as_str()))
            .collect();
        // A temporary identity remains a real carousel tile until replaced by
        // a saved profile. The position after all identities is the actionable
        // create-profile destination.
        let has_temporary = matches!(self.players[slot].profile, LobbyProfileSelection::Temporary);
        let profile_offset = usize::from(has_temporary);
        let new_profile_index = profile_offset + available.len();
        let current = match &self.players[slot].profile {
            LobbyProfileSelection::CreateNew => new_profile_index,
            LobbyProfileSelection::Temporary => 0,
            LobbyProfileSelection::Saved(id) => available
                .iter()
                .position(|profile| profile.id == *id)
                .unwrap_or(0),
        };
        let count = new_profile_index + 1;
        let next = (current as isize + isize::from(direction)).rem_euclid(count as isize) as usize;
        self.players[slot].ready = false;
        self.cancel_launch();
        if next == new_profile_index {
            self.players[slot].profile = LobbyProfileSelection::CreateNew;
        } else if has_temporary && next == 0 {
            self.players[slot].profile = LobbyProfileSelection::Temporary;
        } else if let Some(profile) = available.get(next - profile_offset) {
            self.players[slot].profile = LobbyProfileSelection::Saved(profile.id.clone());
            self.players[slot].display_name = profile.display_name.clone();
        }
    }
}

fn assigned_pattern(device: InputDeviceId, slot_number: usize) -> u8 {
    // Devices receive a stable mixed value for this lobby session. It feels
    // randomly dealt to players without introducing nondeterminism into tests
    // or match replay setup.
    let device_seed = match device {
        InputDeviceId::Mouse => 0x31_u64,
        InputDeviceId::KeyboardPrimary => 0x57,
        InputDeviceId::Gamepad(index) => 0x9b ^ u64::from(index).wrapping_mul(0x045d_9f3b),
    };
    let mixed = device_seed
        .wrapping_add(slot_number as u64 * 0x9e37_79b9)
        .wrapping_mul(0xbf58_476d_1ce4_e5b9);
    ((mixed ^ (mixed >> 29)) % 8) as u8
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
    pub field_seed: u64,
    pub npc_roster_seed: u64,
    pub npc_difficulty: NpcDifficulty,
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

    pub fn advance_rematch(&mut self, replay_same_field: bool) {
        self.replay_same_field = replay_same_field;
        self.npc_roster_seed = self
            .npc_roster_seed
            .wrapping_mul(1442695040888963407)
            .wrapping_add(0x9e37_79b9_7f4a_7c15);
        if !replay_same_field {
            self.field_seed = self
                .field_seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1);
        }
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
            field_seed: 1,
            npc_roster_seed: 0x4e50_4352_4f53_5445,
            npc_difficulty: NpcDifficulty::Normal,
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
    ChangeNpcDifficulty(i8),
    CycleColor(InputDeviceId, i8),
    CycleProfile(InputDeviceId, i8),
}

#[derive(Message, Debug, Clone, Copy)]
pub struct LobbyCommandMessage(pub LobbyCommand);

#[derive(Resource, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LastLobbySettings {
    pub schema_version: u32,
    pub npc_count: u8,
    pub npc_difficulty: NpcDifficulty,
}

impl Default for LastLobbySettings {
    fn default() -> Self {
        Self {
            schema_version: 3,
            npc_count: 0,
            npc_difficulty: NpcDifficulty::Normal,
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
            #[serde(default)]
            npc_difficulty: Option<NpcDifficulty>,
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
            schema_version: 3,
            npc_count: npc_count.min(MAX_COMPETITORS - 1),
            npc_difficulty: stored.npc_difficulty.unwrap_or_default(),
        })
    }
}

pub struct LobbyPlugin;

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MatchLaunchMode {
    #[default]
    StandardCountdown,
    LobbyCountdownCompleted,
}

#[derive(SystemParam)]
struct LobbyLaunch<'w> {
    setup: ResMut<'w, MatchSetup>,
    mode: ResMut<'w, MatchLaunchMode>,
    next: ResMut<'w, NextState<AppState>>,
}

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Lobby>()
            .init_resource::<MatchSetup>()
            .init_resource::<MatchLaunchMode>()
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
                    tick_ready_countdown,
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
        let entry = generate_npc_roster(
            setup.npc_roster_seed ^ u64::from(id.0),
            1,
            setup.npc_difficulty,
        )
        .remove(0);
        commands
            .entity(entity)
            .remove::<HumanController>()
            .insert(NpcController::from_roster(id, &entry, id.index()));
    }
    setup.replace_human_with_npc(device);
    notice.device = None;
    notice.player_name.clear();
    notice.replace_with_npc = false;
    next.set(notice.resume_state);
}

fn restore_lobby_settings(last: Res<LastLobbySettings>, mut lobby: ResMut<Lobby>) {
    lobby.set_npc_count(i16::from(last.npc_count));
    lobby.npc_difficulty = last.npc_difficulty;
    lobby.cancel_launch();
    for player in &mut lobby.players {
        player.ready = false;
    }
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
        (Some(_), MenuAction::Pause) => Some(LobbyCommand::ToggleReady(input.device)),
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
                if let Some(slot) = lobby.slot_for_device(device)
                    && !lobby.players[slot].profile.is_create_new()
                {
                    lobby.players[slot].ready = !lobby.players[slot].ready;
                    lobby.cancel_launch();
                    audio.write(PlayAudioCue::human(AudioCue::Ready));
                }
            }
            LobbyCommand::ChangeNpcCount(delta) => {
                let requested = i16::from(lobby.npc_count()) + i16::from(delta);
                lobby.set_npc_count(requested);
                lobby.cancel_launch();
                last.npc_count = lobby.npc_count();
                persistence.dirty = true;
            }
            LobbyCommand::ChangeNpcDifficulty(delta) => {
                lobby.cycle_npc_difficulty(delta);
                last.npc_difficulty = lobby.npc_difficulty;
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
        }
    }
}

fn tick_ready_countdown(
    time: Res<Time>,
    mut lobby: ResMut<Lobby>,
    mut audio: MessageWriter<PlayAudioCue>,
    mut launch: LobbyLaunch,
) {
    let before = lobby.launch_state.remaining().map(f32::ceil);
    if advance_ready_countdown(&mut lobby, time.delta_secs()) {
        audio.write(PlayAudioCue::human(AudioCue::Countdown));
        prepare_match_setup(&lobby, &mut launch.setup);
        *launch.mode = MatchLaunchMode::LobbyCountdownCompleted;
        audio.write(PlayAudioCue::human(AudioCue::MenuConfirm));
        launch.next.set(AppState::MatchLoading);
    } else if lobby.launch_state.remaining().map(f32::ceil) != before
        && lobby.launch_state.remaining().is_some()
    {
        audio.write(PlayAudioCue::human(AudioCue::Countdown));
    }
}

fn advance_ready_countdown(lobby: &mut Lobby, delta_seconds: f32) -> bool {
    if !lobby.can_start() {
        lobby.cancel_launch();
        return false;
    }
    let remaining = match lobby.launch_state {
        LobbyLaunchState::Waiting => READY_COUNTDOWN_SECONDS,
        LobbyLaunchState::CountingDown(remaining) => remaining,
        LobbyLaunchState::Launching => return false,
    };
    let remaining = (remaining - delta_seconds.max(0.0)).max(0.0);
    if remaining > 0.0 {
        lobby.launch_state = LobbyLaunchState::CountingDown(remaining);
        return false;
    }
    lobby.launch_state = LobbyLaunchState::Launching;
    true
}

fn prepare_match_setup(lobby: &Lobby, setup: &mut MatchSetup) {
    let previous_field_seed = setup.field_seed;
    let previous_roster_seed = setup.npc_roster_seed;
    *setup = MatchSetup {
        field_seed: previous_field_seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1),
        npc_roster_seed: previous_roster_seed
            .wrapping_mul(1442695040888963407)
            .wrapping_add(0x9e37_79b9_7f4a_7c15),
        npc_difficulty: lobby.npc_difficulty,
        total_competitors: lobby.total_competitors(),
        humans: lobby
            .players
            .iter()
            .map(|player| HumanSetup {
                device: player.device,
                profile_id: player.profile.saved_id().map(str::to_owned),
                display_name: player.display_name.clone(),
                color_id: player.color_id,
                pattern_id: player.pattern_id,
            })
            .collect(),
        replay_same_field: false,
    };
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
