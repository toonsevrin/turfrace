//! Shared visual language: bright arcade typography, square controls, and motion.

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
    // Return to Turfrace's original Bungee face: its squared counters and
    // chamfered corners provide the desired block-game influence while its
    // varied arcade geometry keeps it from resembling a Minecraft clone.
    // The built-in mono face remains
    // for compact values and player names where alignment matters.
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
    // Menu decoration is made from the same language as the arena: cropped
    // paper contours and quiet ribbons. Large floating circles read as generic
    // placeholders and have no relationship to the game.
    for (index, (left, top, width, rotation, color)) in [
        (
            percent(7),
            percent(18),
            210.0,
            -0.14,
            Color::srgba(0.12, 0.38, 0.92, 0.16),
        ),
        (
            percent(77),
            percent(24),
            150.0,
            0.18,
            Color::srgba(1.0, 0.28, 0.12, 0.15),
        ),
        (
            percent(10),
            percent(75),
            125.0,
            0.09,
            Color::srgba(0.10, 0.68, 0.34, 0.14),
        ),
        (
            percent(72),
            percent(74),
            235.0,
            -0.10,
            Color::srgba(0.64, 0.18, 0.90, 0.12),
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
                width: px(width),
                height: px(4),
                ..default()
            },
            BackgroundColor(color),
            UiTransform::from_rotation(Rot2::radians(rotation)),
            DecorativeRibbon {
                phase: index as f32 * 1.7,
                speed: 0.18 + index as f32 * 0.025,
                amplitude: 10.0 + index as f32 * 2.0,
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
    // TextShadow can suggest depth, but it cannot outline the upper edges of
    // glyphs. Build headings as a tiny stack of crisp text faces instead: a
    // diagonal charcoal extrusion, a complete ink keyline, a warm bevel, and
    // the paper-white face. This remains ordinary WebGL2-safe UI text and
    // scales cleanly with Bevy's responsive UiScale.
    let depth = heading_depth(size);
    parent
        .spawn((Node {
            width: percent(100),
            height: px(size * 1.34),
            min_height: px(size * 1.34),
            margin: UiRect::bottom(px(4)),
            position_type: PositionType::Relative,
            ..default()
        },))
        .with_children(|heading| {
            for step in (1..=depth).rev() {
                let progress = step as f32 / depth as f32;
                spawn_title_layer(
                    heading,
                    theme,
                    text,
                    size,
                    Vec2::new(((step + 2) / 3) as f32, step as f32),
                    Color::srgb(
                        0.025 + progress * 0.018,
                        0.032 + progress * 0.022,
                        0.045 + progress * 0.028,
                    ),
                );
            }
            let outline = heading_outline(size);
            for direction in [
                Vec2::new(-1.0, -1.0),
                Vec2::new(0.0, -1.0),
                Vec2::new(1.0, -1.0),
                Vec2::new(-1.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(-1.0, 1.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0),
            ] {
                spawn_title_layer(heading, theme, text, size, direction * outline, INK);
            }
            // A warm highlight below the keyline leaves the top edge crisp
            // while making the face feel inset into the dark arcade marquee.
            spawn_title_layer(
                heading,
                theme,
                text,
                size,
                Vec2::new(0.0, 1.0),
                Color::srgb_u8(216, 209, 192),
            );
            spawn_title_layer(
                heading,
                theme,
                text,
                size,
                Vec2::ZERO,
                Color::srgb_u8(250, 247, 237),
            );
        });
}

pub(super) fn heading_depth(size: f32) -> i32 {
    (size * 0.16).round().clamp(5.0, 11.0) as i32
}

pub(super) fn heading_outline(size: f32) -> f32 {
    (size * 0.025).round().clamp(1.0, 2.0)
}

fn spawn_title_layer(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    text: &str,
    size: f32,
    offset: Vec2,
    color: Color,
) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font: theme.display_font.clone(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            left: px(offset.x),
            top: px(offset.y),
            width: percent(100),
            ..default()
        },
        Pickable::IGNORE,
    ));
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
                // Focus is expressed by color and motion, never a detached
                // rule beside the option.
                border: UiRect::all(px(0)),
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
                        font: theme.display_font.clone(),
                        font_size: FontSize::Px(metrics.font_size),
                        ..default()
                    },
                    TextColor(INK),
                    IntegratedButtonLabel {
                        idle: INK,
                        focused: CORAL,
                    },
                ));
            }
        });
}

pub(super) fn spawn_perspective_button_label(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: &str,
    font_size: f32,
) {
    // Primary menu actions use the title's keyline/bevel vocabulary at a
    // shallower depth. Unlike the former white face plus shadow, the complete
    // outline remains readable over both paper and moving territory.
    parent
        .spawn((Node {
            position_type: PositionType::Relative,
            width: percent(100),
            height: px(font_size * 1.38),
            ..default()
        },))
        .with_children(|stack| {
            let depth = button_label_depth(font_size);
            for step in (1..=depth).rev() {
                spawn_button_label_layer(
                    stack,
                    theme,
                    label,
                    font_size,
                    Vec2::new(((step + 1) / 2) as f32, step as f32),
                    INK,
                    INK,
                );
            }
            let outline = button_label_outline(font_size);
            for direction in [
                Vec2::new(-1.0, -1.0),
                Vec2::new(0.0, -1.0),
                Vec2::new(1.0, -1.0),
                Vec2::new(-1.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(-1.0, 1.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0),
            ] {
                spawn_button_label_layer(
                    stack,
                    theme,
                    label,
                    font_size,
                    direction * outline,
                    INK,
                    INK,
                );
            }
            spawn_button_label_layer(
                stack,
                theme,
                label,
                font_size,
                Vec2::new(0.0, 1.0),
                Color::srgb_u8(194, 187, 171),
                CORAL,
            );
            spawn_button_label_layer(
                stack,
                theme,
                label,
                font_size,
                Vec2::ZERO,
                Color::srgb_u8(250, 247, 237),
                CORAL,
            );
        });
}

pub(super) fn button_label_depth(font_size: f32) -> i32 {
    (font_size * 0.16).round().clamp(3.0, 5.0) as i32
}

pub(super) fn button_label_outline(font_size: f32) -> f32 {
    (font_size * 0.055).round().clamp(1.0, 2.0)
}

#[allow(clippy::too_many_arguments)]
fn spawn_button_label_layer(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    label: &str,
    font_size: f32,
    offset: Vec2,
    idle: Color,
    focused: Color,
) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font: theme.display_font.clone(),
            font_size: FontSize::Px(font_size),
            ..default()
        },
        TextColor(idle),
        IntegratedButtonLabel { idle, focused },
        TextLayout::justify(Justify::Left),
        Node {
            position_type: PositionType::Absolute,
            left: px(offset.x),
            top: px(offset.y),
            width: percent(100),
            ..default()
        },
        Pickable::IGNORE,
    ));
}
