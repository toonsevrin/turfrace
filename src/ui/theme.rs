//! Shared visual language: Bungee typography, tight outlined controls, and motion.

use super::*;
use bevy::{core_pipeline::Core2d, render::camera::CameraRenderGraph};

pub(super) fn setup_ui(mut commands: Commands, assets: Res<AssetServer>) {
    let font = FontSource::Handle(assets.load("fonts/Bungee-Regular.ttf"));
    commands.insert_resource(UiTheme { font });
    commands.spawn((
        Camera2d,
        CameraRenderGraph::new(Core2d),
        Camera {
            order: 100,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        IsDefaultUiCamera,
        UiCamera,
    ));
}

pub(super) fn cleanup_screen(
    mut commands: Commands,
    roots: Query<Entity, With<ScreenRoot>>,
    mut focus: ResMut<UiFocus>,
) {
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    focus.entity = None;
}

pub(super) fn screen_node() -> Node {
    Node {
        width: percent(100),
        height: percent(100),
        position_type: PositionType::Absolute,
        display: Display::Flex,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        overflow: Overflow::clip(),
        ..default()
    }
}

pub(super) fn panel_node(width: Val) -> Node {
    Node {
        width,
        max_width: px(940),
        padding: UiRect::axes(px(30), px(24)),
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Stretch,
        row_gap: px(10),
        border: UiRect::all(px(2)),
        // Sharp corners are intentional: this is an arcade control surface,
        // not a rounded web card. The black keyline does the visual grouping.
        border_radius: BorderRadius::all(px(0)),
        ..default()
    }
}

pub(super) fn spawn_background(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Node {
            width: percent(72),
            height: percent(82),
            position_type: PositionType::Absolute,
            left: percent(14),
            top: percent(9),
            border: UiRect::all(px(3)),
            border_radius: BorderRadius::all(px(0)),
            ..default()
        },
        BorderColor::all(Color::srgba(0.30, 0.45, 0.60, 0.12)),
        UiTransform::from_rotation(Rot2::radians(-0.035)),
    ));
    let colors = [CORAL, SKY, LIME, Color::srgb(0.72, 0.35, 0.95)];
    for index in 0..8 {
        parent.spawn((
            Node {
                width: px(150 + (index % 3) * 42),
                height: px(13),
                position_type: PositionType::Absolute,
                left: percent(4.0 + (index * 13 % 82) as f32),
                top: percent(8.0 + (index * 19 % 84) as f32),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(0)),
                ..default()
            },
            BackgroundColor(colors[index % colors.len()].with_alpha(0.16)),
            BorderColor::all(INK.with_alpha(0.42)),
            UiTransform::from_rotation(Rot2::radians(index as f32 * 0.37)),
            DecorativeTrail {
                phase: index as f32 * 0.9,
                speed: 0.45 + index as f32 * 0.035,
                amplitude: 8.0 + (index % 3) as f32 * 5.0,
            },
        ));
    }
}

pub(super) fn spawn_title(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    text: &str,
    size: f32,
) {
    parent
        .spawn((Node {
            width: percent(100),
            min_height: px(size * 1.18),
            margin: UiRect::bottom(px(8)),
            ..default()
        },))
        .with_children(|title| {
            for offset in [
                Vec2::new(-4.0, 0.0),
                Vec2::new(4.0, 0.0),
                Vec2::new(0.0, -4.0),
                Vec2::new(0.0, 4.0),
                Vec2::new(-3.0, -3.0),
                Vec2::new(3.0, -3.0),
                Vec2::new(-3.0, 3.0),
                Vec2::new(3.0, 3.0),
            ] {
                title.spawn((
                    Text::new(text),
                    TextFont {
                        font: theme.font.clone(),
                        font_size: FontSize::Px(size),
                        ..default()
                    },
                    TextColor(INK),
                    TextLayout::justify(Justify::Center),
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        left: px(offset.x),
                        top: px(offset.y),
                        ..default()
                    },
                ));
            }
            title.spawn((
                Text::new(text),
                TextFont {
                    font: theme.font.clone(),
                    font_size: FontSize::Px(size),
                    ..default()
                },
                TextColor(Color::WHITE),
                TextLayout::justify(Justify::Center),
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    ..default()
                },
            ));
        });
}

pub(super) fn spawn_subtitle(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    text: impl Into<String>,
) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font: theme.font.clone(),
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(MUTED),
        TextLayout::justify(Justify::Center),
    ));
}

pub(super) fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: impl Into<String>,
    action: UiAction,
    order: u16,
) {
    spawn_button_sized(
        parent,
        theme,
        label,
        action,
        order,
        ButtonMetrics {
            min_height: 46.0,
            horizontal_padding: 18.0,
            font_size: 20.0,
        },
    );
}

/// A compact control used by dense screens such as the lobby and settings.
/// It still participates in the same FocusOrder path as a normal button.
pub(super) fn spawn_mini_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: impl Into<String>,
    action: UiAction,
    order: u16,
) {
    spawn_button_sized(
        parent,
        theme,
        label,
        action,
        order,
        ButtonMetrics {
            min_height: 38.0,
            horizontal_padding: 10.0,
            font_size: 16.0,
        },
    );
}

#[derive(Clone, Copy)]
struct ButtonMetrics {
    min_height: f32,
    horizontal_padding: f32,
    font_size: f32,
}

fn spawn_button_sized(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: impl Into<String>,
    action: UiAction,
    order: u16,
    metrics: ButtonMetrics,
) {
    parent
        .spawn((
            Button,
            action,
            FocusOrder(order),
            Node {
                min_width: px(58),
                min_height: px(metrics.min_height),
                padding: UiRect::axes(px(metrics.horizontal_padding), px(7)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.095, 0.125, 0.19)),
            BorderColor::all(INK),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font: theme.font.clone(),
                    font_size: FontSize::Px(metrics.font_size),
                    ..default()
                },
                TextColor(Color::WHITE),
                TextShadow {
                    offset: Vec2::new(1.0, 2.0),
                    color: INK,
                },
            ));
        });
}
