use super::*;

#[test]
fn assigned_patterns_are_stable_and_distributed_across_devices() {
    let mouse = assigned_pattern(InputDeviceId::Mouse, 1);
    assert_eq!(mouse, assigned_pattern(InputDeviceId::Mouse, 1));
    assert_ne!(mouse, assigned_pattern(InputDeviceId::KeyboardPrimary, 2));
    assert!(mouse < 8);
}

#[test]
fn clearing_humans_resets_joinable_lobby_state() {
    let profiles = ProfileStore::default();
    let mut lobby = Lobby::default();
    lobby.join(InputDeviceId::Mouse, &profiles);
    lobby.players[0].ready = true;
    lobby.shared_focus_owner = Some(InputDeviceId::Mouse);

    lobby.clear_humans();

    assert!(lobby.players.is_empty());
    assert!(lobby.shared_focus_owner.is_none());
    assert_eq!(lobby.launch_state, LobbyLaunchState::Waiting);
}

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
fn start_requires_two_connected_ready_total_competitors() {
    let profiles = ProfileStore::default();
    let mut lobby = Lobby::default();
    lobby.join(InputDeviceId::Mouse, &profiles);
    lobby.players[0].ready = true;
    assert!(!lobby.can_start());

    lobby.set_npc_count(1);
    assert!(lobby.can_start());

    lobby.players[0].connected = false;
    assert!(!lobby.can_start());
}

#[test]
fn two_ready_humans_can_start_without_robots() {
    let profiles = ProfileStore::default();
    let mut lobby = Lobby::default();
    lobby.join(InputDeviceId::Mouse, &profiles);
    lobby.join(InputDeviceId::KeyboardPrimary, &profiles);
    lobby
        .players
        .iter_mut()
        .for_each(|player| player.ready = true);

    assert!(lobby.can_start());
}

#[test]
fn customization_invalidates_readiness_and_countdown() {
    let profiles = ProfileStore::default();
    let mut lobby = Lobby::default();
    lobby.join(InputDeviceId::Mouse, &profiles);
    lobby.set_npc_count(1);
    lobby.players[0].ready = true;
    assert!(!advance_ready_countdown(&mut lobby, 1.0));

    lobby.cycle_color(0, 1);
    assert!(!lobby.players[0].ready);
    assert_eq!(lobby.launch_state, LobbyLaunchState::Waiting);

    lobby.players[0].ready = true;
    assert!(!advance_ready_countdown(&mut lobby, 1.0));
    lobby.cycle_npc_difficulty(1);
    assert_eq!(lobby.launch_state, LobbyLaunchState::Waiting);
}

#[test]
fn all_ready_starts_one_cancellable_countdown_and_unready_resets_it() {
    let profiles = ProfileStore::default();
    let mut lobby = Lobby::default();
    lobby.join(InputDeviceId::Mouse, &profiles);
    lobby.set_npc_count(1);

    assert!(!advance_ready_countdown(&mut lobby, 10.0));
    assert_eq!(lobby.launch_state, LobbyLaunchState::Waiting);

    lobby.players[0].ready = true;
    assert!(!advance_ready_countdown(&mut lobby, 1.0));
    assert_eq!(lobby.launch_state, LobbyLaunchState::CountingDown(2.0));

    lobby.players[0].ready = false;
    assert!(!advance_ready_countdown(&mut lobby, 1.0));
    assert_eq!(lobby.launch_state, LobbyLaunchState::Waiting);

    lobby.players[0].ready = true;
    assert!(advance_ready_countdown(&mut lobby, 3.0));
    assert_eq!(lobby.launch_state, LobbyLaunchState::Launching);
    assert!(!advance_ready_countdown(&mut lobby, 1.0));
}

#[test]
fn preparing_a_match_commits_players_without_a_second_lobby_countdown() {
    let profiles = ProfileStore::default();
    let mut lobby = Lobby::default();
    lobby.join(InputDeviceId::Mouse, &profiles);
    lobby.players[0].ready = true;
    lobby.set_npc_count(1);
    let mut setup = MatchSetup::default();

    prepare_match_setup(&lobby, &mut setup);

    assert_eq!(setup.humans.len(), 1);
    assert_eq!(setup.humans[0].device, InputDeviceId::Mouse);
    assert_eq!(setup.total_competitors, 2);
    assert_eq!(setup.total_competitors, lobby.total_competitors());
}

#[test]
fn version_one_lobby_preferences_migrate_to_robot_count() {
    let migrated: LastLobbySettings =
        serde_json::from_str(r#"{"schema_version":1,"total_competitors":8}"#).unwrap();
    assert_eq!(migrated.schema_version, 3);
    assert_eq!(migrated.npc_count, 6);
    assert_eq!(migrated.npc_difficulty, NpcDifficulty::Normal);
}

#[test]
fn version_three_lobby_preferences_round_trip() {
    let settings = LastLobbySettings {
        schema_version: 3,
        npc_count: 4,
        npc_difficulty: NpcDifficulty::Hard,
    };
    let encoded = serde_json::to_string(&settings).unwrap();
    assert_eq!(
        serde_json::from_str::<LastLobbySettings>(&encoded).unwrap(),
        settings
    );
}

#[test]
fn version_two_preferences_migrate_to_normal_difficulty() {
    let migrated: LastLobbySettings =
        serde_json::from_str(r#"{"schema_version":2,"npc_count":4}"#).unwrap();
    assert_eq!(migrated.schema_version, 3);
    assert_eq!(migrated.npc_count, 4);
    assert_eq!(migrated.npc_difficulty, NpcDifficulty::Normal);
}

#[test]
fn replay_field_only_preserves_the_field_seed() {
    let mut setup = MatchSetup::default();
    let field = setup.field_seed;
    let roster = setup.npc_roster_seed;
    setup.advance_rematch(true);
    assert_eq!(setup.field_seed, field);
    assert_ne!(setup.npc_roster_seed, roster);

    let roster = setup.npc_roster_seed;
    setup.advance_rematch(false);
    assert_ne!(setup.field_seed, field);
    assert_ne!(setup.npc_roster_seed, roster);
}

#[test]
fn profile_carousel_includes_an_actionable_new_profile_tile() {
    let mut profiles = ProfileStore::default();
    profiles.create("Ada");
    let mut lobby = Lobby::default();
    lobby.join(InputDeviceId::Mouse, &profiles);

    lobby.cycle_profile(0, 1, &profiles);
    assert!(lobby.players[0].profile.is_create_new());
    assert!(!lobby.players[0].ready);

    lobby.cycle_profile(0, 1, &profiles);
    assert!(!lobby.players[0].profile.is_create_new());
    assert_eq!(lobby.players[0].display_name, "Ada");
}

#[test]
fn new_profile_tile_cannot_become_match_ready() {
    let profiles = ProfileStore::default();
    let mut lobby = Lobby::default();
    let device = InputDeviceId::Mouse;
    lobby.join(device, &profiles);
    lobby.set_npc_count(1);
    lobby.cycle_profile(0, 1, &profiles);
    assert!(lobby.players[0].profile.is_create_new());
    // Even a stale or programmatically supplied ready flag cannot launch
    // the create-profile carousel tile as a competitor identity.
    lobby.players[0].ready = true;
    assert!(!lobby.can_start());
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
