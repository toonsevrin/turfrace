//! Device-independent menu and steering input.

use std::collections::{HashMap, HashSet};

use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::profiles::UserSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputDeviceId {
    Gamepad(u32),
    Mouse,
    KeyboardPrimary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlSource {
    Gamepad,
    Mouse,
    Keyboard,
    Npc,
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct SteeringIntent {
    pub desired_direction: Vec2,
    pub magnitude: f32,
    pub source: ControlSource,
}

impl Default for SteeringIntent {
    fn default() -> Self {
        Self {
            desired_direction: Vec2::Y,
            magnitude: 0.0,
            source: ControlSource::Keyboard,
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanController {
    pub device: InputDeviceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Join,
    Confirm,
    Secondary,
    Back,
    Pause,
    Up,
    Down,
    Left,
    Right,
    ColorPrevious,
    ColorNext,
}

#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuInput {
    pub device: InputDeviceId,
    pub action: MenuAction,
}

#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceDisconnected(pub InputDeviceId);

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastActiveDevice(pub InputDeviceId);

impl Default for LastActiveDevice {
    fn default() -> Self {
        Self(InputDeviceId::KeyboardPrimary)
    }
}

#[derive(Resource, Debug, Default)]
pub struct ConnectedDevices {
    gamepads: HashSet<u32>,
}

#[derive(Resource, Debug, Default)]
struct GamepadMenuLatch(HashSet<u32>);

impl ConnectedDevices {
    pub fn is_connected(&self, device: InputDeviceId) -> bool {
        match device {
            InputDeviceId::Gamepad(id) => self.gamepads.contains(&id),
            InputDeviceId::Mouse | InputDeviceId::KeyboardPrimary => true,
        }
    }
}

/// Camera/presentation code can update a mouse player's aim point without
/// coupling movement to camera implementation details.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct MouseAimWorld(pub Option<Vec2>);

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConnectedDevices>()
            .init_resource::<LastActiveDevice>()
            .init_resource::<MouseAimWorld>()
            .init_resource::<GamepadMenuLatch>()
            .add_message::<MenuInput>()
            .add_message::<DeviceDisconnected>()
            .add_systems(
                PreUpdate,
                (track_gamepads, poll_keyboard_and_mouse, poll_gamepad_menus).chain(),
            )
            .add_systems(Update, build_human_steering_intents);
    }
}

fn track_gamepads(
    gamepads: Query<Entity, With<Gamepad>>,
    mut connected: ResMut<ConnectedDevices>,
    mut disconnected: MessageWriter<DeviceDisconnected>,
) {
    let now: HashSet<u32> = gamepads
        .iter()
        .map(|entity| entity.index().index())
        .collect();
    for id in connected.gamepads.difference(&now) {
        disconnected.write(DeviceDisconnected(InputDeviceId::Gamepad(*id)));
    }
    connected.gamepads = now;
}

fn poll_keyboard_and_mouse(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut input: MessageWriter<MenuInput>,
    mut last: ResMut<LastActiveDevice>,
) {
    let mut send = |device, action| {
        input.write(MenuInput { device, action });
        last.0 = device;
    };

    if keys.any_just_pressed([KeyCode::Enter, KeyCode::Space]) {
        send(InputDeviceId::KeyboardPrimary, MenuAction::Confirm);
    }
    if keys.just_pressed(KeyCode::Escape) {
        send(InputDeviceId::KeyboardPrimary, MenuAction::Back);
    }
    for (key, action) in [
        (KeyCode::ArrowUp, MenuAction::Up),
        (KeyCode::KeyW, MenuAction::Up),
        (KeyCode::ArrowDown, MenuAction::Down),
        (KeyCode::KeyS, MenuAction::Down),
        (KeyCode::ArrowLeft, MenuAction::Left),
        (KeyCode::KeyA, MenuAction::Left),
        (KeyCode::ArrowRight, MenuAction::Right),
        (KeyCode::KeyD, MenuAction::Right),
    ] {
        if keys.just_pressed(key) {
            send(InputDeviceId::KeyboardPrimary, action);
        }
    }
    if mouse.just_pressed(MouseButton::Left) {
        send(InputDeviceId::Mouse, MenuAction::Join);
    }
}

fn poll_gamepad_menus(
    gamepads: Query<(Entity, &Gamepad)>,
    mut input: MessageWriter<MenuInput>,
    mut last: ResMut<LastActiveDevice>,
    mut latch: ResMut<GamepadMenuLatch>,
) {
    for (entity, gamepad) in &gamepads {
        let device = InputDeviceId::Gamepad(entity.index().index());
        let mut send = |action| {
            input.write(MenuInput { device, action });
            last.0 = device;
        };
        if gamepad.just_pressed(GamepadButton::South) {
            send(MenuAction::Confirm);
        }
        if gamepad.any_just_pressed([GamepadButton::North, GamepadButton::West]) {
            send(MenuAction::Secondary);
        }
        if gamepad.just_pressed(GamepadButton::East) {
            send(MenuAction::Back);
        }
        if gamepad.just_pressed(GamepadButton::Start) {
            send(MenuAction::Pause);
        }
        for (button, action) in [
            (GamepadButton::DPadUp, MenuAction::Up),
            (GamepadButton::DPadDown, MenuAction::Down),
            (GamepadButton::DPadLeft, MenuAction::Left),
            (GamepadButton::DPadRight, MenuAction::Right),
            (GamepadButton::LeftTrigger, MenuAction::ColorPrevious),
            (GamepadButton::RightTrigger, MenuAction::ColorNext),
        ] {
            if gamepad.just_pressed(button) {
                send(action);
            }
        }
        let stick = gamepad.left_stick();
        if latch.0.contains(&entity.index().index()) {
            if stick.length() < 0.45 {
                latch.0.remove(&entity.index().index());
            }
        } else {
            let action = if stick.y > 0.7 {
                Some(MenuAction::Up)
            } else if stick.y < -0.7 {
                Some(MenuAction::Down)
            } else if stick.x < -0.7 {
                Some(MenuAction::Left)
            } else if stick.x > 0.7 {
                Some(MenuAction::Right)
            } else {
                None
            };
            if let Some(action) = action {
                send(action);
                latch.0.insert(entity.index().index());
            }
        }
    }
}

fn build_human_steering_intents(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mouse_aim: Res<MouseAimWorld>,
    settings: Res<UserSettings>,
    mut humans: Query<(&HumanController, &mut SteeringIntent)>,
) {
    let by_id: HashMap<u32, &Gamepad> = gamepads
        .iter()
        .map(|(entity, gamepad)| (entity.index().index(), gamepad))
        .collect();

    for (controller, mut intent) in &mut humans {
        let (raw, source) = match controller.device {
            InputDeviceId::Gamepad(id) => (
                by_id.get(&id).map_or(Vec2::ZERO, |gamepad| {
                    let left = gamepad.left_stick();
                    let right = gamepad.right_stick();
                    screen_direction_to_world(if right.length_squared() > left.length_squared() {
                        right
                    } else {
                        left
                    })
                }),
                ControlSource::Gamepad,
            ),
            InputDeviceId::KeyboardPrimary => {
                let x = i8::from(keys.pressed(KeyCode::KeyD)) as f32
                    - i8::from(keys.pressed(KeyCode::KeyA)) as f32;
                let y = i8::from(keys.pressed(KeyCode::KeyS)) as f32
                    - i8::from(keys.pressed(KeyCode::KeyW)) as f32;
                (Vec2::new(x, y).normalize_or_zero(), ControlSource::Keyboard)
            }
            InputDeviceId::Mouse => (mouse_aim.0.unwrap_or(Vec2::ZERO), ControlSource::Mouse),
        };
        let processed = if source == ControlSource::Gamepad {
            radial_deadzone(raw, settings.gamepad_deadzone)
        } else {
            raw.clamp_length_max(1.0)
        };
        if processed.length_squared() > 0.0 {
            intent.desired_direction = processed.normalize();
        }
        intent.magnitude = processed.length();
        intent.source = source;
    }
}

pub fn radial_deadzone(input: Vec2, deadzone: f32) -> Vec2 {
    let length = input.length();
    if length <= deadzone {
        return Vec2::ZERO;
    }
    let magnitude = ((length.min(1.0) - deadzone) / (1.0 - deadzone)).clamp(0.0, 1.0);
    input.normalize_or_zero() * magnitude
}

/// Cameras are north-locked from +Z, so screen-up is logical -Y.
pub fn screen_direction_to_world(screen: Vec2) -> Vec2 {
    Vec2::new(screen.x, -screen.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radial_deadzone_is_radial_and_rescaled() {
        assert_eq!(radial_deadzone(Vec2::new(0.1, 0.1), 0.18), Vec2::ZERO);
        let edge = radial_deadzone(Vec2::new(1.0, 0.0), 0.18);
        assert!((edge.x - 1.0).abs() < 0.0001);
        let diagonal = radial_deadzone(Vec2::splat(0.5), 0.18);
        assert!((diagonal.x - diagonal.y).abs() < 0.0001);
    }

    #[test]
    fn screen_up_maps_to_world_negative_y() {
        assert_eq!(screen_direction_to_world(Vec2::Y), Vec2::NEG_Y);
        assert_eq!(screen_direction_to_world(Vec2::X), Vec2::X);
    }
}
