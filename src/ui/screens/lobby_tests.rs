use super::*;

#[test]
fn controller_focus_exposes_every_card_customization_command() {
    let device = InputDeviceId::Gamepad(7);
    let actions = card_customization_actions(device);
    assert!(matches!(actions[0], LobbyCommand::CycleProfile(d, -1) if d == device));
    assert!(matches!(actions[1], LobbyCommand::CycleProfile(d, 1) if d == device));
    assert!(matches!(actions[2], LobbyCommand::CycleColor(d, -1) if d == device));
    assert!(matches!(actions[3], LobbyCommand::CycleColor(d, 1) if d == device));
}

#[test]
fn lobby_keeps_controls_visible_at_browser_stress_widths() {
    assert!(is_compact_lobby(800.0));
    assert!(!is_compact_lobby(960.0));
    assert!(!is_compact_lobby(1280.0));
}

#[test]
fn lobby_scroll_is_clamped_to_content_bounds() {
    assert_eq!(clamp_lobby_scroll(0.0, -100.0, 900.0, 600.0, 1.0), 0.0);
    assert_eq!(clamp_lobby_scroll(100.0, 500.0, 900.0, 600.0, 1.0), 300.0);
    assert_eq!(clamp_lobby_scroll(0.0, 100.0, 500.0, 600.0, 1.0), 0.0);
}

#[test]
fn empty_join_beacon_is_a_mouse_join_action() {
    assert!(matches!(
        join_beacon_action(),
        UiAction::Lobby(LobbyCommand::Join(InputDeviceId::Mouse))
    ));
}

#[test]
fn countdown_ticks_do_not_change_the_structural_fingerprint() {
    let mut lobby = Lobby::default();
    lobby.launch_state = crate::lobby::LobbyLaunchState::CountingDown(2.9);
    let first = lobby_fingerprint(&lobby, false);
    lobby.launch_state = crate::lobby::LobbyLaunchState::CountingDown(1.1);
    assert_eq!(lobby_fingerprint(&lobby, false), first);
    assert_eq!(countdown_display(2.9), 3);
    assert_eq!(countdown_display(1.1), 2);
}

#[test]
fn fresh_two_player_lobby_starts_without_robots() {
    let profiles = ProfileStore::default();
    let mut lobby = Lobby::default();
    lobby.join(InputDeviceId::Mouse, &profiles);
    lobby.join(InputDeviceId::KeyboardPrimary, &profiles);
    assert_eq!(lobby.npc_count(), 0);
    assert_eq!(lobby.max_npc_count(), 10);
}
