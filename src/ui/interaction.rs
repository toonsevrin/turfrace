//! Focus, activation, settings mutation, profile editing, and pause controls.

use super::*;
use bevy::ecs::system::SystemParam;

type ChangedActionButtons<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static Interaction, &'static UiAction),
    (Changed<Interaction>, With<Button>),
>;

#[derive(SystemParam)]
pub(super) struct UiActionResources<'w> {
    state: Res<'w, State<AppState>>,
    next: ResMut<'w, NextState<AppState>>,
    settings_return: ResMut<'w, SettingsReturn>,
    settings: ResMut<'w, UserSettings>,
    profiles: ResMut<'w, ProfileStore>,
    lobby: ResMut<'w, Lobby>,
    lobby_commands: MessageWriter<'w, LobbyCommandMessage>,
    setup: ResMut<'w, MatchSetup>,
    disconnect: ResMut<'w, MatchDisconnectNotice>,
    persistence: ResMut<'w, PersistenceStatus>,
    confirmation: ResMut<'w, Confirmation>,
    editor: ResMut<'w, NameEditor>,
}
pub(super) fn animate_background(
    time: Res<Time>,
    settings: Res<UserSettings>,
    mut trails: Query<(&DecorativeTrail, &mut UiTransform)>,
) {
    if settings.reduced_motion {
        return;
    }
    for (trail, mut transform) in &mut trails {
        let t = time.elapsed_secs() * trail.speed + trail.phase;
        transform.translation = Val2::px(
            t.sin() * trail.amplitude,
            (t * 0.73).cos() * trail.amplitude * 0.5,
        );
    }
}

pub(super) fn button_interactions(
    mut buttons: ChangedActionButtons,
    mut focus: ResMut<UiFocus>,
    mut activated: MessageWriter<UiActivated>,
    mut audio: MessageWriter<PlayAudioCue>,
) {
    for (entity, interaction, action) in &mut buttons {
        match interaction {
            Interaction::Pressed => {
                focus.entity = Some(entity);
                activated.write(UiActivated(action.clone()));
                audio.write(PlayAudioCue::human(audio_cue_for_action(action)));
            }
            Interaction::Hovered => focus.entity = Some(entity),
            Interaction::None => {}
        }
    }
}

pub(super) fn controller_navigation(
    mut inputs: MessageReader<MenuInput>,
    state: Res<State<AppState>>,
    buttons: Query<(Entity, &FocusOrder, &UiAction), With<Button>>,
    mut focus: ResMut<UiFocus>,
    mut activated: MessageWriter<UiActivated>,
    mut audio: MessageWriter<PlayAudioCue>,
    editor: Res<NameEditor>,
) {
    let mut ordered: Vec<_> = buttons.iter().collect();
    ordered.sort_by_key(|(_, order, _)| order.0);
    if ordered.is_empty() {
        return;
    }
    if focus.entity.is_none() {
        focus.entity = Some(ordered[0].0);
    }
    for input in inputs.read() {
        match input.action {
            MenuAction::Up | MenuAction::Left => {
                let current = ordered
                    .iter()
                    .position(|(entity, _, _)| Some(*entity) == focus.entity)
                    .unwrap_or(0);
                focus.entity = Some(ordered[(current + ordered.len() - 1) % ordered.len()].0);
                audio.write(PlayAudioCue::human(AudioCue::MenuMove));
            }
            MenuAction::Down | MenuAction::Right => {
                let current = ordered
                    .iter()
                    .position(|(entity, _, _)| Some(*entity) == focus.entity)
                    .unwrap_or(0);
                focus.entity = Some(ordered[(current + 1) % ordered.len()].0);
                audio.write(PlayAudioCue::human(AudioCue::MenuMove));
            }
            MenuAction::Confirm | MenuAction::Secondary => {
                if !activates_focused_button(
                    *state.get(),
                    input.action,
                    editor.target.is_some(),
                    input.device,
                ) {
                    continue;
                }
                if let Some((_, _, action)) = ordered
                    .iter()
                    .find(|(entity, _, _)| Some(*entity) == focus.entity)
                {
                    activated.write(UiActivated((*action).clone()));
                    audio.write(PlayAudioCue::human(audio_cue_for_action(action)));
                }
            }
            _ => {}
        }
    }
}

fn activates_focused_button(
    state: AppState,
    action: MenuAction,
    editing_name: bool,
    device: InputDeviceId,
) -> bool {
    action == MenuAction::Secondary
        || (action == MenuAction::Confirm
            && (state != AppState::Lobby
                || (editing_name && matches!(device, InputDeviceId::Gamepad(_)))))
}

pub(super) fn audio_cue_for_action(action: &UiAction) -> AudioCue {
    match action {
        UiAction::Resume => AudioCue::Resume,
        UiAction::SettingsBack | UiAction::EditorCancel => AudioCue::MenuBack,
        _ => AudioCue::MenuConfirm,
    }
}

pub(super) fn update_button_focus(
    focus: Res<UiFocus>,
    settings: Res<UserSettings>,
    mut buttons: Query<
        (Entity, &Interaction, &mut BackgroundColor, &mut BorderColor),
        With<Button>,
    >,
) {
    if !focus.is_changed()
        && !buttons
            .iter()
            .any(|(_, interaction, _, _)| *interaction == Interaction::Pressed)
    {
        return;
    }
    for (entity, interaction, mut background, mut border) in &mut buttons {
        let selected = focus.entity == Some(entity) || *interaction == Interaction::Hovered;
        background.0 = if selected {
            SKY
        } else if settings.high_contrast_ui {
            Color::BLACK
        } else {
            Color::srgb(0.095, 0.125, 0.19)
        };
        border.set_all(if selected { Color::WHITE } else { INK });
    }
}

pub(super) fn dispatch_ui_actions(
    mut commands: Commands,
    mut activated: MessageReader<UiActivated>,
    resources: UiActionResources,
    theme: Res<UiTheme>,
    editor_overlays: Query<Entity, With<NameEditorOverlay>>,
) {
    let UiActionResources {
        state,
        mut next,
        mut settings_return,
        mut settings,
        mut profiles,
        mut lobby,
        mut lobby_commands,
        mut setup,
        mut disconnect,
        mut persistence,
        mut confirmation,
        mut editor,
    } = resources;
    for message in activated.read() {
        match &message.0 {
            UiAction::State(target) => next.set(*target),
            UiAction::Settings => {
                settings_return.0 = *state.get();
                next.set(AppState::Settings);
            }
            UiAction::SettingsBack => next.set(settings_return.0),
            UiAction::Resume => next.set(AppState::Playing),
            UiAction::ReplaceDisconnected => {
                if disconnect.device.is_some() {
                    disconnect.replace_with_npc = true;
                }
            }
            UiAction::Lobby(command) => {
                lobby_commands.write(LobbyCommandMessage(*command));
            }
            UiAction::AdjustSetting(field, amount) => {
                let value = match field {
                    SettingField::Master => &mut settings.master_volume,
                    SettingField::Music => &mut settings.music_volume,
                    SettingField::SoundEffects => &mut settings.sound_effect_volume,
                    SettingField::ScreenShake => &mut settings.screen_shake,
                    SettingField::GamepadDeadzone => &mut settings.gamepad_deadzone,
                    SettingField::MouseSensitivity => &mut settings.mouse_sensitivity,
                    _ => continue,
                };
                let (minimum, maximum) = match field {
                    SettingField::GamepadDeadzone => (0.05, 0.50),
                    SettingField::MouseSensitivity => (0.50, 2.0),
                    _ => (0.0, 1.0),
                };
                *value = (*value + amount).clamp(minimum, maximum);
                settings.normalize();
                persistence.dirty = true;
            }
            UiAction::ToggleSetting(field) => {
                match field {
                    SettingField::ReducedMotion => {
                        settings.reduced_motion = !settings.reduced_motion
                    }
                    SettingField::Colorblind => {
                        settings.colorblind_assist = !settings.colorblind_assist
                    }
                    SettingField::Fullscreen => settings.fullscreen = !settings.fullscreen,
                    SettingField::LargerHudText => {
                        settings.larger_hud_text = !settings.larger_hud_text
                    }
                    SettingField::HighContrast => {
                        settings.high_contrast_ui = !settings.high_contrast_ui
                    }
                    SettingField::GraphicsQuality => {
                        settings.graphics_quality = match settings.graphics_quality {
                            GraphicsQualitySetting::Low => GraphicsQualitySetting::Medium,
                            GraphicsQualitySetting::Medium => GraphicsQualitySetting::High,
                            GraphicsQualitySetting::High => GraphicsQualitySetting::Low,
                        }
                    }
                    _ => continue,
                }
                persistence.dirty = true;
            }
            UiAction::ResetStatistics => {
                if confirmation.action == Some(ConfirmationAction::ResetStatistics) {
                    profiles.reset_statistics();
                    confirmation.action = None;
                    persistence.dirty = true;
                } else {
                    confirmation.action = Some(ConfirmationAction::ResetStatistics);
                    confirmation.remaining = 4.0;
                }
            }
            UiAction::ResetAllData => {
                if confirmation.action == Some(ConfirmationAction::ResetAllData) {
                    *profiles = ProfileStore::default();
                    *settings = UserSettings::default();
                    *lobby = Lobby::default();
                    confirmation.action = None;
                    persistence.dirty = true;
                } else {
                    confirmation.action = Some(ConfirmationAction::ResetAllData);
                    confirmation.remaining = 4.0;
                }
            }
            UiAction::CreateProfile(slot) => {
                if *slot >= lobby.players.len() {
                    continue;
                }
                let default_name = format!("Player {}", slot + 1);
                let profile = profiles.create(&default_name).clone();
                lobby.players[*slot].profile_id = Some(profile.id);
                lobby.players[*slot].display_name = profile.display_name.clone();
                editor.target = Some(NameEditorTarget::LobbySlot(*slot));
                editor.buffer = profile.display_name;
                for entity in &editor_overlays {
                    commands.entity(entity).despawn();
                }
                spawn_name_editor(&mut commands, &theme, &editor.buffer);
                persistence.dirty = true;
            }
            UiAction::RenameProfile(profile_id) => {
                if let Some(profile) = profiles
                    .profiles
                    .iter()
                    .find(|profile| profile.id == *profile_id)
                {
                    editor.target = Some(NameEditorTarget::ExistingProfile(profile_id.clone()));
                    editor.buffer = profile.display_name.clone();
                    for entity in &editor_overlays {
                        commands.entity(entity).despawn();
                    }
                    spawn_name_editor(&mut commands, &theme, &editor.buffer);
                }
            }
            UiAction::DeleteProfile(profile_id) => {
                let pending = ConfirmationAction::DeleteProfile(profile_id.clone());
                if confirmation.action.as_ref() == Some(&pending) {
                    profiles.remove(profile_id);
                    detach_profile_from_lobby(&mut lobby, profile_id);
                    confirmation.action = None;
                    persistence.dirty = true;
                } else {
                    confirmation.action = Some(pending);
                    confirmation.remaining = 4.0;
                }
            }
            UiAction::EditorCharacter(character) => {
                if editor.buffer.chars().count() < 16 {
                    editor.buffer.push(*character);
                }
            }
            UiAction::EditorBackspace => {
                editor.buffer.pop();
            }
            UiAction::EditorSave => {
                if let Some(target) = editor.target.clone() {
                    let profile_id = match &target {
                        NameEditorTarget::LobbySlot(slot) => lobby
                            .players
                            .get(*slot)
                            .and_then(|player| player.profile_id.clone()),
                        NameEditorTarget::ExistingProfile(profile_id) => Some(profile_id.clone()),
                    };
                    if let Some(profile_id) = profile_id
                        && let Some(profile) = profiles
                            .profiles
                            .iter_mut()
                            .find(|profile| profile.id == profile_id)
                    {
                        profile.display_name =
                            sanitize_profile_name(&editor.buffer, &profile.display_name);
                        for player in &mut lobby.players {
                            if player.profile_id.as_deref() == Some(&profile_id) {
                                player.display_name = profile.display_name.clone();
                            }
                        }
                    }
                }
                editor.target = None;
                for entity in &editor_overlays {
                    commands.entity(entity).despawn();
                }
                persistence.dirty = true;
            }
            UiAction::EditorCancel => {
                editor.target = None;
                for entity in &editor_overlays {
                    commands.entity(entity).despawn();
                }
            }
            UiAction::Rematch { same_field } => {
                setup.replay_same_field = *same_field;
                if !same_field {
                    setup.seed = setup.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                }
                next.set(AppState::MatchLoading);
            }
        }
    }
}

fn detach_profile_from_lobby(lobby: &mut Lobby, profile_id: &str) {
    for player in &mut lobby.players {
        if player.profile_id.as_deref() == Some(profile_id) {
            player.profile_id = None;
        }
    }
}

pub(super) fn spawn_name_editor(commands: &mut Commands, theme: &UiTheme, value: &str) {
    commands
        .spawn((
            NameEditorOverlay,
            Node {
                width: percent(100),
                height: percent(100),
                position_type: PositionType::Absolute,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.02, 0.03, 0.86)),
            GlobalZIndex(200),
        ))
        .with_children(|overlay| {
            let mut editor_panel = panel_node(percent(94));
            editor_panel.max_width = px(710);
            overlay
                .spawn((editor_panel, BackgroundColor(PANEL), BorderColor::all(INK)))
                .with_children(|panel| {
                    spawn_title(panel, theme, "NEW PROFILE", 35.0);
                    panel.spawn((
                        Text::new(value),
                        TextFont {
                            font: theme.font.clone(),
                            font_size: FontSize::Px(28.0),
                            ..default()
                        },
                        TextColor(LIME),
                        TextLayout::justify(Justify::Center),
                        NameEditorValue,
                    ));
                    panel
                        .spawn((Node {
                            display: Display::Grid,
                            grid_template_columns: RepeatedGridTrack::flex(10, 1.0),
                            row_gap: px(5),
                            column_gap: px(5),
                            ..default()
                        },))
                        .with_children(|keyboard| {
                            for (index, character) in
                                "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars().enumerate()
                            {
                                spawn_key(
                                    keyboard,
                                    theme,
                                    character.to_string(),
                                    UiAction::EditorCharacter(character),
                                    200 + index as u16,
                                );
                            }
                        });
                    panel
                        .spawn((Node {
                            display: Display::Flex,
                            column_gap: px(8),
                            ..default()
                        },))
                        .with_children(|row| {
                            spawn_button(row, theme, "BACKSPACE", UiAction::EditorBackspace, 240);
                            spawn_button(row, theme, "SAVE", UiAction::EditorSave, 241);
                            spawn_button(row, theme, "CANCEL", UiAction::EditorCancel, 242);
                        });
                });
        });
}

pub(super) fn spawn_key(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: String,
    action: UiAction,
    order: u16,
) {
    parent
        .spawn((
            Button,
            action,
            FocusOrder(order),
            Node {
                width: px(52),
                height: px(45),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.095, 0.125, 0.19)),
            BorderColor::all(INK),
        ))
        .with_children(|key| {
            key.spawn((
                Text::new(label),
                TextFont {
                    font: theme.font.clone(),
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
        });
}

pub(super) fn edit_profile_name(
    mut keyboard: MessageReader<KeyboardInput>,
    mut editor: ResMut<NameEditor>,
    mut texts: Query<&mut Text, With<NameEditorValue>>,
    mut activated: MessageWriter<UiActivated>,
) {
    if editor.target.is_none() {
        return;
    }
    for event in keyboard.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match event.key_code {
            KeyCode::Backspace => {
                editor.buffer.pop();
            }
            KeyCode::Enter => {
                activated.write(UiActivated(UiAction::EditorSave));
            }
            KeyCode::Escape => {
                activated.write(UiActivated(UiAction::EditorCancel));
            }
            _ => {
                if let Some(value) = &event.text {
                    for character in value.chars().filter(|character| !character.is_control()) {
                        if editor.buffer.chars().count() < 16 {
                            editor.buffer.push(character);
                        }
                    }
                }
            }
        }
    }
    if editor.is_changed() {
        for mut text in &mut texts {
            text.0 = editor.buffer.clone();
        }
    }
}

pub(super) fn tick_confirmation(time: Res<Time>, mut confirmation: ResMut<Confirmation>) {
    if confirmation.action.is_none() {
        return;
    }
    confirmation.remaining -= time.delta_secs();
    if confirmation.remaining <= 0.0 {
        confirmation.action = None;
    }
}

pub(super) fn pause_input(
    mut input: MessageReader<MenuInput>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut audio: MessageWriter<PlayAudioCue>,
    settings_return: Res<SettingsReturn>,
) {
    for input in input.read() {
        match (*state.get(), input.action) {
            (AppState::Playing, MenuAction::Pause | MenuAction::Back) => {
                next.set(AppState::Paused);
                audio.write(PlayAudioCue::human(AudioCue::Pause));
            }
            (AppState::Paused, MenuAction::Pause | MenuAction::Back) => {
                next.set(AppState::Playing);
                audio.write(PlayAudioCue::human(AudioCue::Resume));
            }
            (AppState::LocalLeaderboard, MenuAction::Back) => next.set(AppState::Home),
            (AppState::Settings, MenuAction::Back) => next.set(settings_return.0),
            _ => {}
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputDeviceId;

    #[test]
    fn deleting_a_selected_profile_preserves_slot_but_detaches_identity() {
        let mut profiles = ProfileStore::default();
        let profile_id = profiles.create("Ada").id.clone();
        let mut lobby = Lobby::default();
        lobby.join(InputDeviceId::Mouse, &profiles);
        assert_eq!(
            lobby.players[0].profile_id.as_deref(),
            Some(profile_id.as_str())
        );

        assert!(profiles.remove(&profile_id));
        detach_profile_from_lobby(&mut lobby, &profile_id);
        assert_eq!(lobby.players.len(), 1);
        assert!(lobby.players[0].profile_id.is_none());
        assert_eq!(lobby.players[0].display_name, "Ada");
    }

    #[test]
    fn lobby_secondary_activates_controls_while_south_remains_ready() {
        assert!(activates_focused_button(
            AppState::Lobby,
            MenuAction::Secondary,
            false,
            InputDeviceId::Gamepad(1),
        ));
        assert!(!activates_focused_button(
            AppState::Lobby,
            MenuAction::Confirm,
            false,
            InputDeviceId::Gamepad(1),
        ));
        assert!(activates_focused_button(
            AppState::Lobby,
            MenuAction::Confirm,
            true,
            InputDeviceId::Gamepad(1),
        ));
        assert!(!activates_focused_button(
            AppState::Lobby,
            MenuAction::Confirm,
            true,
            InputDeviceId::KeyboardPrimary,
        ));
    }
}
