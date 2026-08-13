//! Device registration cards and match-composition controls.

use super::super::*;
use crate::lobby::MAX_HUMANS;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::window::PrimaryWindow;

fn card_customization_actions(device: InputDeviceId) -> [LobbyCommand; 4] {
    [
        LobbyCommand::CycleProfile(device, -1),
        LobbyCommand::CycleProfile(device, 1),
        LobbyCommand::CycleColor(device, -1),
        LobbyCommand::CycleColor(device, 1),
    ]
}

fn spawn_selector_arrow(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    direction: &str,
    command: LobbyCommand,
    order: u16,
    _accent: Color,
) {
    // Interactive controls use one deliberate coral focus language. Card
    // colors identify players without creating pale, glitch-like button fills.
    let accent = CORAL;
    parent
        .spawn((
            Button,
            IntegratedMenuButton,
            LobbySelectorButton { accent },
            UiAction::Lobby(command),
            FocusOrder(order),
            Node {
                width: px(38),
                height: px(38),
                border: UiRect::all(px(1)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(INK.with_alpha(0.22)),
            UiTransform::default(),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(direction),
                TextFont {
                    font: theme.display_font.clone(),
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(INK),
                IntegratedButtonLabel {
                    idle: INK,
                    focused: Color::WHITE,
                },
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_icon_stepper(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: &str,
    center: impl FnOnce(&mut ChildSpawnerCommands),
    previous: LobbyCommand,
    next: LobbyCommand,
    order: u16,
    accent: Color,
) {
    parent
        .spawn((Node {
            width: percent(100),
            min_height: px(if label.is_empty() { 68 } else { 62 }),
            flex_direction: FlexDirection::Column,
            justify_content: if label.is_empty() {
                JustifyContent::Center
            } else {
                JustifyContent::FlexStart
            },
            row_gap: px(5),
            ..default()
        },))
        .with_children(|group| {
            if !label.is_empty() {
                group.spawn((
                    Text::new(label),
                    TextFont {
                        font: theme.body_font.clone(),
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(MUTED),
                    TextLayout::justify(Justify::Center),
                    Node {
                        width: percent(100),
                        ..default()
                    },
                ));
            }
            group
                .spawn((Node {
                    width: percent(100),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: px(8),
                    ..default()
                },))
                .with_children(|row| {
                    spawn_selector_arrow(row, theme, "◀", previous, order, accent);
                    row.spawn((
                        Node {
                            width: px(230),
                            height: px(46),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            row_gap: px(1),
                            border: UiRect::bottom(px(2)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                        BorderColor::all(INK.with_alpha(0.12)),
                    ))
                    .with_children(center);
                    spawn_selector_arrow(row, theme, "▶", next, order + 1, accent);
                });
        });
}

fn spawn_device_icon(parent: &mut ChildSpawnerCommands, device: InputDeviceId, color: Color) {
    let (width, height, radius) = match device {
        InputDeviceId::Mouse => (22.0, 30.0, 9.0),
        InputDeviceId::KeyboardPrimary => (38.0, 24.0, 2.0),
        InputDeviceId::Gamepad(_) => (38.0, 24.0, 10.0),
    };
    parent
        .spawn((
            Node {
                width: px(width),
                height: px(height),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(radius)),
                position_type: PositionType::Relative,
                ..default()
            },
            BorderColor::all(color),
        ))
        .with_children(|icon| match device {
            InputDeviceId::Mouse => {
                icon.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(8),
                        top: px(3),
                        width: px(2),
                        height: px(8),
                        ..default()
                    },
                    BackgroundColor(color),
                ));
            }
            InputDeviceId::KeyboardPrimary => {
                for top in [5.0, 12.0] {
                    icon.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(5),
                            top: px(top),
                            width: px(24),
                            height: px(2),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                }
            }
            InputDeviceId::Gamepad(_) => {
                icon.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(7),
                        top: px(6),
                        width: px(3),
                        height: px(9),
                        ..default()
                    },
                    BackgroundColor(color),
                ));
                icon.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(4),
                        top: px(9),
                        width: px(9),
                        height: px(3),
                        ..default()
                    },
                    BackgroundColor(color),
                ));
                for left in [24.0, 29.0] {
                    icon.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(left),
                            top: px(9),
                            width: px(3),
                            height: px(3),
                            border_radius: BorderRadius::all(percent(50)),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                }
            }
        });
}

fn spawn_lobby_back_button(parent: &mut ChildSpawnerCommands, theme: &UiTheme) {
    parent
        .spawn((
            Button,
            IntegratedMenuButton,
            UiAction::Back(AppState::Home),
            FocusOrder(25),
            Node {
                position_type: PositionType::Absolute,
                left: px(28),
                top: px(24),
                width: px(138),
                min_height: px(48),
                padding: UiRect::axes(px(4), px(6)),
                border: UiRect::all(px(0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
            UiTransform::default(),
            GlobalZIndex(4),
        ))
        .with_children(|button| {
            spawn_perspective_button_label(button, theme, "BACK", 20.0);
        });
}

fn join_beacon_action() -> UiAction {
    UiAction::Lobby(LobbyCommand::Join(InputDeviceId::Mouse))
}

fn spawn_join_beacon(parent: &mut ChildSpawnerCommands, theme: &UiTheme) {
    parent
        .spawn((
            Button,
            join_beacon_action(),
            FocusOrder(19),
            Node {
                width: percent(100),
                max_width: px(590),
                min_height: px(146),
                padding: UiRect::axes(px(30), px(22)),
                border: UiRect::all(px(0)),
                align_self: AlignSelf::Center,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                ..default()
            },
            BackgroundColor(PAPER.with_alpha(0.90)),
            BorderColor::all(Color::NONE),
            UiTransform::default(),
            LobbyJoinBeacon { phase: 0.0 },
            Interaction::default(),
        ))
        .with_children(|beacon| {
            beacon
                .spawn((Node {
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    column_gap: px(18),
                    ..default()
                },))
                .with_children(|icons| {
                    spawn_device_icon(icons, InputDeviceId::KeyboardPrimary, MUTED);
                    icons.spawn((
                        Text::new("•"),
                        TextFont {
                            font: theme.display_font.clone(),
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(CORAL),
                    ));
                    spawn_device_icon(icons, InputDeviceId::Gamepad(0), MUTED);
                    icons.spawn((
                        Text::new("•"),
                        TextFont {
                            font: theme.display_font.clone(),
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(CORAL),
                    ));
                    spawn_device_icon(icons, InputDeviceId::Mouse, MUTED);
                });
            beacon.spawn((
                Text::new("CLAIM A RACER"),
                TextFont {
                    font: theme.display_font.clone(),
                    font_size: FontSize::Px(23.0),
                    ..default()
                },
                TextColor(INK),
                TextLayout::justify(Justify::Center),
            ));
            beacon.spawn((
                Text::new("BUTTON  /  KEY  /  CLICK"),
                TextFont {
                    font: theme.body_font.clone(),
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(MUTED),
                TextLayout::justify(Justify::Center),
            ));
        });
}

fn spawn_robot_icon(parent: &mut ChildSpawnerCommands, color: Color) {
    parent
        .spawn((Node {
            width: px(30),
            height: px(28),
            position_type: PositionType::Relative,
            ..default()
        },))
        .with_children(|robot| {
            robot.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(5),
                    top: px(7),
                    width: px(20),
                    height: px(16),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BorderColor::all(color),
            ));
            robot.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(14),
                    top: px(1),
                    width: px(2),
                    height: px(7),
                    ..default()
                },
                BackgroundColor(color),
            ));
            for left in [10.0, 18.0] {
                robot.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(left),
                        top: px(12),
                        width: px(3),
                        height: px(3),
                        border_radius: BorderRadius::all(percent(50)),
                        ..default()
                    },
                    BackgroundColor(color),
                ));
            }
        });
}

pub(crate) fn spawn_lobby(
    mut commands: Commands,
    theme: Res<UiTheme>,
    lobby: Res<Lobby>,
    mut fingerprint: ResMut<LobbyFingerprint>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let compact = windows
        .single()
        .is_ok_and(|window| is_compact_lobby(window.width()));
    fingerprint.0 = lobby_fingerprint(&lobby, compact);
    spawn_lobby_content(&mut commands, &theme, &lobby, compact);
}

pub(crate) fn spawn_lobby_content(
    commands: &mut Commands,
    theme: &UiTheme,
    lobby: &Lobby,
    compact: bool,
) {
    commands
        .spawn((
            ScreenRoot,
            screen_node(),
            BackgroundColor(Color::srgba(0.025, 0.035, 0.055, 0.30)),
        ))
        .with_children(|root| {
            spawn_lobby_back_button(root, theme);
            let mut lobby_panel = panel_node(percent(90));
            lobby_panel.max_width = px(1040);
            lobby_panel.max_height = percent(96);
            lobby_panel.padding = UiRect::axes(px(22), px(10));
            lobby_panel.row_gap = px(9);
            lobby_panel.overflow = Overflow::scroll_y();
            root.spawn((
                lobby_panel,
                BackgroundColor(Color::NONE),
                BorderColor::all(INK),
                LobbyScrollContainer,
                ScrollPosition::default(),
            ))
            .with_children(|panel| {
                spawn_title(panel, theme, "PICK YOUR RACER", 38.0);
                if lobby.players.is_empty() {
                    spawn_join_beacon(panel, theme);
                }
                panel
                    .spawn((Node {
                        width: percent(100),
                        display: Display::Grid,
                        grid_template_columns: if compact {
                            RepeatedGridTrack::minmax(
                                1,
                                MinTrackSizingFunction::Px(0.0),
                                MaxTrackSizingFunction::Fraction(1.0),
                            )
                        } else {
                            RepeatedGridTrack::minmax(
                                2,
                                MinTrackSizingFunction::Px(0.0),
                                MaxTrackSizingFunction::Fraction(1.0),
                            )
                        },
                        column_gap: px(18),
                        row_gap: px(18),
                        ..default()
                    },))
                    .with_children(|grid| {
                        for (slot, player) in lobby.players.iter().enumerate() {
                            let [previous_profile, next_profile, previous_color, next_color] =
                                card_customization_actions(player.device);
                            let color = palette_color(player.color_id);
                            grid.spawn((
                                Node {
                                    min_height: px(268),
                                    padding: UiRect::new(px(18), px(18), px(34), px(18)),
                                    border: UiRect::all(px(0)),
                                    position_type: PositionType::Relative,
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(18),
                                    ..default()
                                },
                                BackgroundColor(PAPER.with_alpha(0.96)),
                                BorderColor::all(Color::NONE),
                                UiTransform::default(),
                                LobbyCardVisual {
                                    color,
                                    phase: slot as f32 * 1.7,
                                    ready: player.ready,
                                },
                            ))
                            .with_children(|card| {
                                card.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: px(0),
                                        top: px(-1),
                                        width: percent(100),
                                        height: px(8),
                                        ..default()
                                    },
                                    BackgroundColor(color),
                                ));
                                card.spawn((
                                    Text::new(format!("P{}", slot + 1)),
                                    TextFont {
                                        font: theme.display_font.clone(),
                                        font_size: FontSize::Px(10.0),
                                        ..default()
                                    },
                                    TextColor(color),
                                    Node {
                                        position_type: PositionType::Absolute,
                                        right: px(14),
                                        top: px(13),
                                        ..default()
                                    },
                                ));
                                card.spawn((Node {
                                    width: percent(100),
                                    flex_grow: 1.0,
                                    flex_direction: FlexDirection::Column,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    row_gap: px(18),
                                    ..default()
                                },))
                                    .with_children(|controls| {
                                        spawn_icon_stepper(
                                            controls,
                                            theme,
                                            "",
                                            |center| {
                                                let bundle = (
                                                    Text::new(if player.profile.is_create_new() {
                                                        "+  NEW PROFILE".to_owned()
                                                    } else {
                                                        player.display_name.clone()
                                                    }),
                                                    TextFont {
                                                        font: theme.display_font.clone(),
                                                        font_size: FontSize::Px(19.0),
                                                        ..default()
                                                    },
                                                    TextColor(if player.profile.is_create_new() {
                                                        color
                                                    } else {
                                                        INK
                                                    }),
                                                    TextLayout::justify(Justify::Center),
                                                    Node {
                                                        width: px(210),
                                                        ..default()
                                                    },
                                                    UiTransform::default(),
                                                );
                                                if player.profile.is_create_new() {
                                                    center.spawn((
                                                        bundle,
                                                        Button,
                                                        UiAction::CreateProfile(slot),
                                                        FocusOrder(46 + slot as u16 * 10),
                                                    ));
                                                } else {
                                                    center.spawn(bundle);
                                                }
                                            },
                                            previous_profile,
                                            next_profile,
                                            40 + slot as u16 * 10,
                                            color,
                                        );
                                        spawn_icon_stepper(
                                            controls,
                                            theme,
                                            "",
                                            |center| {
                                                center.spawn((
                                                    Node {
                                                        width: px(36),
                                                        height: px(36),
                                                        border: UiRect::all(px(2)),
                                                        ..default()
                                                    },
                                                    BackgroundColor(color),
                                                    BorderColor::all(INK),
                                                ));
                                            },
                                            previous_color,
                                            next_color,
                                            42 + slot as u16 * 10,
                                            color,
                                        );
                                    });
                                if player.connected {
                                    card.spawn((
                                        Button,
                                        UiAction::Lobby(LobbyCommand::ToggleReady(player.device)),
                                        FocusOrder(48 + slot as u16 * 10),
                                        ReadyPrompt {
                                            color,
                                            phase: slot as f32 * 1.7,
                                            ready: player.ready,
                                        },
                                        Node {
                                            width: percent(100),
                                            min_height: px(68),
                                            display: Display::Flex,
                                            align_items: AlignItems::Center,
                                            justify_content: JustifyContent::SpaceBetween,
                                            padding: UiRect::axes(px(20), px(10)),
                                            column_gap: px(12),
                                            border: UiRect::all(px(2)),
                                            ..default()
                                        },
                                        BackgroundColor(color.with_alpha(0.10)),
                                        BorderColor::all(color.with_alpha(0.72)),
                                        UiTransform::default(),
                                    ))
                                    .with_children(|ready| {
                                        spawn_device_icon(ready, player.device, color);
                                        ready
                                            .spawn((Node {
                                                flex_direction: FlexDirection::Column,
                                                align_items: AlignItems::FlexEnd,
                                                row_gap: px(2),
                                                ..default()
                                            },))
                                            .with_children(|copy| {
                                                copy.spawn((
                                                    Text::new(if player.ready {
                                                        "LOCKED IN"
                                                    } else {
                                                        "READY UP"
                                                    }),
                                                    TextFont {
                                                        font: theme.display_font.clone(),
                                                        font_size: FontSize::Px(23.0),
                                                        ..default()
                                                    },
                                                    TextColor(if player.ready {
                                                        color
                                                    } else {
                                                        INK
                                                    }),
                                                ));
                                                copy.spawn((
                                                    Text::new(if player.ready {
                                                        "PRESS TO EDIT"
                                                    } else {
                                                        "CONFIRM"
                                                    }),
                                                    TextFont {
                                                        font: theme.body_font.clone(),
                                                        font_size: FontSize::Px(9.0),
                                                        ..default()
                                                    },
                                                    TextColor(MUTED),
                                                ));
                                            });
                                    });
                                } else {
                                    card.spawn((
                                        Text::new("PRESS ANY BUTTON TO RECLAIM"),
                                        TextFont {
                                            font: theme.body_font.clone(),
                                            font_size: FontSize::Px(13.0),
                                            ..default()
                                        },
                                        TextColor(CORAL),
                                        TextLayout::justify(Justify::Center),
                                        Node {
                                            width: percent(100),
                                            ..default()
                                        },
                                    ));
                                }
                            });
                        }
                    });
                if !lobby.players.is_empty()
                    && lobby.players.len() < MAX_HUMANS
                    && lobby.slot_for_device(InputDeviceId::Mouse).is_none()
                {
                    spawn_button(
                        panel,
                        theme,
                        "CLAIM WITH MOUSE",
                        UiAction::Lobby(LobbyCommand::Join(InputDeviceId::Mouse)),
                        19,
                    );
                }
                panel
                    .spawn((
                        Node {
                            width: percent(94),
                            max_width: px(700),
                            min_height: px(64),
                            display: Display::Flex,
                            align_items: AlignItems::Stretch,
                            justify_content: JustifyContent::SpaceBetween,
                            align_self: AlignSelf::Center,
                            position_type: PositionType::Relative,
                            border: UiRect::all(px(0)),
                            ..default()
                        },
                        BackgroundColor(PAPER.with_alpha(0.82)),
                        BorderColor::all(Color::NONE),
                    ))
                    .with_children(|bar| {
                        bar.spawn((Node {
                            width: percent(50),
                            padding: UiRect::axes(px(16), px(10)),
                            column_gap: px(10),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::FlexStart,
                            ..default()
                        },))
                            .with_children(|bots| {
                                spawn_robot_icon(bots, MUTED);
                                bots.spawn((
                                    Text::new("RIVAL BOTS"),
                                    TextFont {
                                        font: theme.display_font.clone(),
                                        font_size: FontSize::Px(13.0),
                                        ..default()
                                    },
                                    TextColor(INK),
                                ));
                                spawn_selector_arrow(
                                    bots,
                                    theme,
                                    "−",
                                    LobbyCommand::ChangeNpcCount(-1),
                                    20,
                                    MUTED,
                                );
                                bots.spawn((
                                    Text::new(format!("{:02}", lobby.npc_count())),
                                    TextFont {
                                        font: theme.display_font.clone(),
                                        font_size: FontSize::Px(24.0),
                                        ..default()
                                    },
                                    TextColor(CORAL),
                                    Node {
                                        width: px(42),
                                        ..default()
                                    },
                                    TextLayout::justify(Justify::Center),
                                ));
                                spawn_selector_arrow(
                                    bots,
                                    theme,
                                    "+",
                                    LobbyCommand::ChangeNpcCount(1),
                                    21,
                                    MUTED,
                                );
                            });
                        bar.spawn((Node {
                            width: percent(50),
                            padding: UiRect::axes(px(16), px(10)),
                            column_gap: px(10),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::FlexEnd,
                            ..default()
                        },))
                            .with_children(|difficulty| {
                                difficulty.spawn((
                                    Text::new("DIFFICULTY"),
                                    TextFont {
                                        font: theme.display_font.clone(),
                                        font_size: FontSize::Px(12.0),
                                        ..default()
                                    },
                                    TextColor(INK),
                                ));
                                spawn_selector_arrow(
                                    difficulty,
                                    theme,
                                    "◀",
                                    LobbyCommand::ChangeNpcDifficulty(-1),
                                    22,
                                    MUTED,
                                );
                                difficulty.spawn((
                                    Text::new(lobby.npc_difficulty.label()),
                                    TextFont {
                                        font: theme.display_font.clone(),
                                        font_size: FontSize::Px(12.0),
                                        ..default()
                                    },
                                    TextColor(INK),
                                    Node {
                                        width: px(78),
                                        ..default()
                                    },
                                    TextLayout::justify(Justify::Center),
                                ));
                                spawn_selector_arrow(
                                    difficulty,
                                    theme,
                                    "▶",
                                    LobbyCommand::ChangeNpcDifficulty(1),
                                    23,
                                    MUTED,
                                );
                            });
                    });
            });
            if let Some(remaining) = lobby.launch_state.remaining() {
                spawn_lobby_countdown(root, theme, remaining);
            }
        });
}

fn spawn_lobby_countdown(parent: &mut ChildSpawnerCommands, theme: &UiTheme, remaining: f32) {
    parent
        .spawn((
            LobbyCountdownOverlay,
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                width: px(146),
                height: px(146),
                margin: UiRect::new(px(-73), px(0), px(-73), px(0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(4)),
                ..default()
            },
            BackgroundColor(CORAL),
            BorderColor::all(INK),
            UiTransform::from_rotation(Rot2::radians(std::f32::consts::FRAC_PI_4)),
            BoxShadow::new(
                Color::srgba(0.02, 0.03, 0.05, 0.42),
                px(9),
                px(11),
                px(0),
                px(0),
            ),
            GlobalZIndex(20),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(112),
                        height: px(112),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(2)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(255, 111, 88)),
                    BorderColor::all(Color::WHITE.with_alpha(0.72)),
                ))
                .with_children(|face| {
                    face.spawn((
                        Text::new(format!("{}", countdown_display(remaining))),
                        LobbyCountdownText,
                        TextFont {
                            font: theme.display_font.clone(),
                            font_size: FontSize::Px(74.0),
                            ..default()
                        },
                        TextColor(Color::srgb_u8(250, 247, 237)),
                        TextShadow {
                            offset: Vec2::new(5.0, 7.0),
                            color: INK,
                        },
                        UiTransform::from_rotation(Rot2::radians(-std::f32::consts::FRAC_PI_4)),
                    ));
                });
        });
}

fn countdown_display(remaining: f32) -> u8 {
    remaining.ceil().max(1.0) as u8
}

pub(crate) fn update_lobby_countdown(
    lobby: Res<Lobby>,
    mut labels: Query<&mut Text, With<LobbyCountdownText>>,
) {
    let Some(remaining) = lobby.launch_state.remaining() else {
        return;
    };
    let value = countdown_display(remaining).to_string();
    for mut label in &mut labels {
        if label.as_str() != value {
            **label = value.clone();
        }
    }
}

pub(crate) fn lobby_fingerprint(lobby: &Lobby, compact: bool) -> String {
    let players = lobby
        .players
        .iter()
        .map(|player| {
            format!(
                "{:?}:{}:{}:{}:{}:{}:{}:{}",
                player.device,
                player.display_name,
                player.color_id,
                player.pattern_id,
                player.profile.is_create_new(),
                player.ready,
                player.connected,
                player.profile.saved_id().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    format!(
        "{}:{:?}:{:?}:{}:{players}",
        lobby.npc_count(),
        lobby.npc_difficulty,
        lobby.launch_state.remaining().is_some(),
        compact
    )
}

const fn is_compact_lobby(width: f32) -> bool {
    // Two cards still fit comfortably at the 960px browser stress width. A
    // premature one-column switch made the primary start/back controls fall
    // below the viewport, so reserve stacking for genuinely narrow canvases.
    width < 840.0
}

#[derive(Component)]
pub(crate) struct LobbyScrollContainer;

pub(crate) fn scroll_lobby(
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut scroll: Query<(&mut ScrollPosition, &Node, &ComputedNode), With<LobbyScrollContainer>>,
) {
    let Ok((mut position, node, computed)) = scroll.single_mut() else {
        return;
    };
    let mut delta = 0.0;
    for event in mouse_wheel.read() {
        let amount = match event.unit {
            MouseScrollUnit::Line => event.y * 24.0,
            MouseScrollUnit::Pixel => event.y,
        };
        delta -= amount;
    }
    if delta == 0.0 || node.overflow.y != OverflowAxis::Scroll {
        return;
    }
    position.y = clamp_lobby_scroll(
        position.y,
        delta,
        computed.content_size().y,
        computed.size().y,
        computed.inverse_scale_factor,
    );
}

fn clamp_lobby_scroll(
    current: f32,
    delta: f32,
    content_height: f32,
    viewport_height: f32,
    inverse_scale_factor: f32,
) -> f32 {
    let max_offset = (content_height - viewport_height).max(0.0) * inverse_scale_factor;
    (current + delta).clamp(0.0, max_offset)
}

pub(crate) fn update_lobby_screen(
    mut commands: Commands,
    theme: Res<UiTheme>,
    lobby: Res<Lobby>,
    mut fingerprint: ResMut<LobbyFingerprint>,
    mut focus: ResMut<UiFocus>,
    roots: Query<Entity, With<ScreenRoot>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let compact = windows
        .single()
        .is_ok_and(|window| is_compact_lobby(window.width()));
    let next = lobby_fingerprint(&lobby, compact);
    if next == fingerprint.0 {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    focus.entity = None;
    fingerprint.0 = next;
    spawn_lobby_content(&mut commands, &theme, &lobby, compact);
}

#[cfg(test)]
#[path = "lobby_tests.rs"]
mod tests;
