//! Shared visual language: Bungee typography, tight outlined controls, and motion.

use super::*;
use bevy::{core_pipeline::Core2d, render::camera::CameraRenderGraph, window::PrimaryWindow};

const UI_REFERENCE_WIDTH: f32 = 1280.0;
const UI_REFERENCE_HEIGHT: f32 = 720.0;
const UI_MIN_SCALE: f32 = 1.0;
const UI_MAX_SCALE: f32 = 3.0;

/// The menus are authored against a 1280×720 logical reference canvas. Using
/// the smaller axis preserves the composition on ultrawide windows without
/// stretching typography, while the lower clamp keeps 960×600 stress layouts
/// from becoming needlessly small.
pub(super) fn ui_scale_for_viewport(width: f32, height: f32) -> f32 {
    if width <= 0.0 || height <= 0.0 {
        return 1.0;
    }
    (width / UI_REFERENCE_WIDTH)
        .min(height / UI_REFERENCE_HEIGHT)
        .clamp(UI_MIN_SCALE, UI_MAX_SCALE)
}

pub(super) fn update_ui_scale(
    mut ui_scale: ResMut<UiScale>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(window) = windows.single() else { return };
    let next = ui_scale_for_viewport(window.width(), window.height());
    if (ui_scale.0 - next).abs() > f32::EPSILON {
        ui_scale.0 = next;
    }
}

pub(super) fn setup_ui(
    mut commands: Commands,
    assets: Res<AssetServer>,
    presentation: Res<crate::render::PresentationSettings>,
) {
    let display_font = FontSource::Handle(assets.load("fonts/Bungee-Regular.ttf"));
    // Keep the expressive display face for titles. The built-in mono face is
    // intentionally used for body copy: it is available in native and WASM
    // builds and keeps stats, controls, and player names aligned.
    let body_font = FontSource::default();
    commands.insert_resource(UiTheme {
        display_font,
        body_font,
    });
    commands.spawn((
        Camera2d,
        presentation.msaa(),
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
        padding: UiRect::axes(px(32), px(26)),
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Stretch,
        row_gap: px(12),
        border: UiRect::all(px(0)),
        border_radius: BorderRadius::all(px(0)),
        ..default()
    }
}

pub(super) fn spawn_background(parent: &mut ChildSpawnerCommands) {
    // Soft moving turf marks give every screen the same visual vocabulary as
    // play without placing the menu on a rectangular panel.
    for (index, (left, top, size, color)) in [
        (
            percent(8),
            percent(13),
            150.0,
            Color::srgba(0.12, 0.38, 0.92, 0.08),
        ),
        (
            percent(82),
            percent(18),
            96.0,
            Color::srgba(1.0, 0.28, 0.12, 0.10),
        ),
        (
            percent(12),
            percent(72),
            78.0,
            Color::srgba(0.10, 0.68, 0.34, 0.09),
        ),
        (
            percent(76),
            percent(70),
            180.0,
            Color::srgba(0.64, 0.18, 0.90, 0.07),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left,
                top,
                width: px(size),
                height: px(size),
                border_radius: BorderRadius::all(percent(50)),
                ..default()
            },
            BackgroundColor(color),
            UiTransform::default(),
            DecorativeTrail {
                phase: index as f32 * 1.7,
                speed: 0.18 + index as f32 * 0.025,
                amplitude: 14.0 + index as f32 * 2.0,
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
            min_height: px(size * 1.1),
            margin: UiRect::bottom(px(4)),
            ..default()
        },))
        .with_children(|title| {
            // A black keyline keeps the pale face legible on paper; the
            // down-right layers give it a small printed, dimensional edge.
            for offset in [
                Vec2::new(-2.0, 0.0),
                Vec2::new(2.0, 0.0),
                Vec2::new(0.0, -2.0),
                Vec2::new(0.0, 2.0),
                Vec2::new(2.0, 2.0),
                Vec2::new(4.0, 4.0),
            ] {
                title.spawn((
                    Text::new(text),
                    TextFont {
                        font: theme.display_font.clone(),
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
                    font: theme.display_font.clone(),
                    font_size: FontSize::Px(size),
                    ..default()
                },
                TextColor(CREAM),
                TextLayout::justify(Justify::Center),
                Node {
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
            font: theme.body_font.clone(),
            font_size: FontSize::Px(14.0),
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
            width: percent(76),
            max_width: px(360),
            align_self: AlignSelf::Center,
            label_style: ButtonLabelStyle::Perspective,
            frame_style: ButtonFrameStyle::MenuRule,
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
            min_height: 36.0,
            horizontal_padding: 12.0,
            font_size: 14.0,
            width: Val::Auto,
            max_width: Val::Auto,
            align_self: AlignSelf::Start,
            label_style: ButtonLabelStyle::Utility,
            frame_style: ButtonFrameStyle::Box,
        },
    );
}

/// A short menu action for dense screens such as the results table. It keeps
/// the same centered measure as primary menu actions without adding height.
pub(super) fn spawn_compact_menu_button(
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
            min_height: 34.0,
            horizontal_padding: 10.0,
            font_size: 15.0,
            width: Val::Auto,
            max_width: Val::Auto,
            align_self: AlignSelf::Center,
            label_style: ButtonLabelStyle::Utility,
            frame_style: ButtonFrameStyle::QuietRule,
        },
    );
}

#[derive(Clone, Copy)]
struct ButtonMetrics {
    min_height: f32,
    horizontal_padding: f32,
    font_size: f32,
    width: Val,
    max_width: Val,
    align_self: AlignSelf,
    label_style: ButtonLabelStyle,
    frame_style: ButtonFrameStyle,
}

#[derive(Clone, Copy)]
enum ButtonLabelStyle {
    Perspective,
    Utility,
}

#[derive(Clone, Copy)]
enum ButtonFrameStyle {
    MenuRule,
    Box,
    QuietRule,
}

fn spawn_button_sized(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: impl Into<String>,
    action: UiAction,
    order: u16,
    metrics: ButtonMetrics,
) {
    let label = label.into();
    // Absolute text layers do not contribute to flex sizing. Compact controls
    // therefore receive a stable text-derived measure while menu actions use
    // their explicit centered width.
    let intrinsic_width = (label.chars().count() as f32 * metrics.font_size * 0.62
        + metrics.horizontal_padding * 2.0
        + 8.0)
        .max(58.0);
    let width = if matches!(metrics.width, Val::Auto) {
        px(intrinsic_width)
    } else {
        metrics.width
    };
    let justify_content = match metrics.frame_style {
        ButtonFrameStyle::MenuRule => JustifyContent::FlexStart,
        ButtonFrameStyle::Box | ButtonFrameStyle::QuietRule => JustifyContent::Center,
    };
    parent
        .spawn((
            Button,
            IntegratedMenuButton,
            action,
            FocusOrder(order),
            Node {
                width,
                max_width: metrics.max_width,
                min_width: px(58),
                min_height: px(metrics.min_height),
                padding: UiRect::axes(px(metrics.horizontal_padding), px(7)),
                // Reserve a stable three-pixel color key so focus never
                // changes the control's measure or introduces a panel.
                border: UiRect::left(px(3)),
                align_self: metrics.align_self,
                justify_content,
                align_items: AlignItems::Center,
                ..default()
            },
            UiTransform::default(),
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
        ))
        .with_children(|button| match metrics.label_style {
            ButtonLabelStyle::Perspective => {
                spawn_perspective_button_label(button, theme, &label, metrics.font_size);
            }
            ButtonLabelStyle::Utility => {
                button.spawn((
                    Text::new(label),
                    TextFont {
                        font: theme.body_font.clone(),
                        font_size: FontSize::Px(metrics.font_size),
                        ..default()
                    },
                    TextColor(INK),
                    IntegratedButtonLabel { idle: INK },
                ));
            }
        });
}

fn spawn_perspective_button_label(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: &str,
    font_size: f32,
) {
    parent
        .spawn((Node {
            width: percent(100),
            height: px(font_size * 1.25),
            position_type: PositionType::Relative,
            ..default()
        },))
        .with_children(|stack| {
            for offset in [
                Vec2::new(-2.0, 0.0),
                Vec2::new(2.0, 0.0),
                Vec2::new(0.0, -2.0),
                Vec2::new(0.0, 2.0),
                Vec2::new(2.0, 2.0),
                Vec2::new(3.0, 3.0),
            ] {
                stack.spawn((
                    Text::new(label),
                    TextFont {
                        font: theme.display_font.clone(),
                        font_size: FontSize::Px(font_size),
                        ..default()
                    },
                    TextColor(INK),
                    TextLayout::justify(Justify::Left),
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        left: px(offset.x),
                        top: px(offset.y),
                        ..default()
                    },
                ));
            }
            stack.spawn((
                Text::new(label),
                TextFont {
                    font: theme.display_font.clone(),
                    font_size: FontSize::Px(font_size),
                    ..default()
                },
                TextColor(CREAM),
                IntegratedButtonLabel { idle: CREAM },
                TextLayout::justify(Justify::Left),
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    ..default()
                },
            ));
        });
}
