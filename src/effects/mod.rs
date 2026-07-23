//! Cosmetic event presentation. Effects are deliberately non-authoritative.

use std::collections::HashMap;

use bevy::{prelude::*, sprite::Text2dShadow};

use crate::{
    camera::{CameraFovPulse, CameraTuning, PlayerCamera},
    match_game::KillProgress,
    palette::{PLAYER_COLORS, palette_color},
    render::{FlatMaterial, PresentationSettings},
};

#[derive(Message, Debug, Clone)]
pub enum VisualEffect {
    Capture {
        source: Entity,
        position: Vec2,
        color_id: u8,
        percent: f32,
    },
    Death {
        source: Entity,
        position: Vec2,
        color_id: u8,
    },
    Kill {
        source: Entity,
        position: Vec2,
        color_id: u8,
        progress: KillProgress,
    },
    Respawn {
        source: Entity,
        position: Vec2,
        color_id: u8,
    },
    LeaderChanged {
        position: Vec2,
        color_id: u8,
    },
    Victory {
        position: Vec2,
        color_id: u8,
    },
    TrailCut {
        position: Vec2,
        color_id: u8,
    },
}

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<VisualEffect>()
            .add_systems(Startup, setup_effect_assets)
            .add_systems(
                Update,
                (spawn_effects, update_effects, apply_camera_shake).chain(),
            );
    }
}

#[derive(Resource)]
struct EffectAssets {
    fragment: Handle<Mesh>,
    ring: Handle<Mesh>,
    popup_font: FontSource,
    materials: Vec<Handle<FlatMaterial>>,
}

#[derive(Component)]
struct EffectLifetime {
    remaining: f32,
    total: f32,
}
#[derive(Component)]
struct EffectVelocity(Vec3);
#[derive(Component)]
struct RisingPopup;
#[derive(Component, Default)]
struct CameraTrauma {
    amount: f32,
    phase: f32,
}

fn setup_effect_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FlatMaterial>>,
) {
    let materials = PLAYER_COLORS
        .iter()
        .map(|color| materials.add(FlatMaterial::new(Color::Srgba(*color), 0.12)))
        .collect();
    commands.insert_resource(EffectAssets {
        fragment: meshes.add(Cuboid::from_size(Vec3::splat(0.16))),
        // A flat graphic ring reads like an arcade pulse from every camera.
        // The old lit 3D torus looked like a dark crater when viewed at the
        // gameplay camera angle.
        ring: meshes.add(Annulus::new(0.64, 0.76).mesh().resolution(32)),
        popup_font: FontSource::Handle(asset_server.load("fonts/Bungee-Regular.ttf")),
        materials,
    });
}

fn spawn_effects(
    mut commands: Commands,
    mut events: MessageReader<VisualEffect>,
    assets: Res<EffectAssets>,
    tuning: Res<CameraTuning>,
    settings: Res<PresentationSettings>,
    cameras: Query<(Entity, &PlayerCamera)>,
) {
    let camera_by_subject: HashMap<Entity, Entity> =
        cameras.iter().map(|(e, c)| (c.subject, e)).collect();
    for event in events.read() {
        let (position, color_id) = match *event {
            VisualEffect::Capture {
                position, color_id, ..
            }
            | VisualEffect::Death {
                position, color_id, ..
            }
            | VisualEffect::Kill {
                position, color_id, ..
            }
            | VisualEffect::Respawn {
                position, color_id, ..
            }
            | VisualEffect::LeaderChanged { position, color_id }
            | VisualEffect::Victory { position, color_id }
            | VisualEffect::TrailCut { position, color_id } => (position, color_id),
        };
        let palette = color_id as usize % assets.materials.len();
        match *event {
            VisualEffect::Kill {
                source,
                position,
                color_id,
                progress,
            } => {
                spawn_kill_effect(
                    &mut commands,
                    &assets,
                    &tuning,
                    &settings,
                    &camera_by_subject,
                    source,
                    position,
                    color_id,
                    palette,
                    progress,
                );
            }
            VisualEffect::Death { .. } | VisualEffect::TrailCut { .. } => {
                for index in 0..14 {
                    let angle =
                        index as f32 * std::f32::consts::TAU / 14.0 + (index % 3) as f32 * 0.13;
                    let speed = 2.4 + (index % 5) as f32 * 0.45;
                    commands.spawn((
                        Mesh3d(assets.fragment.clone()),
                        MeshMaterial3d(assets.materials[palette].clone()),
                        Transform::from_xyz(position.x, 0.72, position.y),
                        EffectVelocity(Vec3::new(
                            angle.cos() * speed,
                            2.3 + index as f32 * 0.08,
                            angle.sin() * speed,
                        )),
                        EffectLifetime {
                            remaining: 0.55,
                            total: 0.55,
                        },
                    ));
                }
                if let VisualEffect::Death { source, .. } = *event
                    && let Some(&camera) = camera_by_subject.get(&source)
                {
                    commands.entity(camera).insert(CameraTrauma {
                        amount: 1.0,
                        phase: 0.0,
                    });
                }
            }
            VisualEffect::Capture { percent, .. } => {
                // Corridor-sized NPC captures happen in bursts and used to
                // create a knot of near-zero labels in every split viewport.
                // Keep the celebration for captures a player can actually
                // read and understand; particles still acknowledge tiny ones.
                if let Some(label) = capture_popup_label(percent) {
                    commands.spawn((
                        Text2d::new(label),
                        TextFont {
                            font: assets.popup_font.clone(),
                            font_size: FontSize::Px(24.0),
                            ..default()
                        },
                        TextColor(palette_color(color_id)),
                        Text2dShadow {
                            offset: Vec2::new(2.0, -2.0),
                            color: Color::srgba(0.01, 0.015, 0.025, 0.92),
                        },
                        Transform::from_xyz(position.x, 2.0, position.y),
                        RisingPopup,
                        EffectLifetime {
                            remaining: 0.85,
                            total: 0.85,
                        },
                    ));
                }
                for index in 0..8 {
                    let angle = index as f32 * std::f32::consts::TAU / 8.0;
                    commands.spawn((
                        Mesh3d(assets.fragment.clone()),
                        MeshMaterial3d(assets.materials[palette].clone()),
                        Transform::from_xyz(
                            position.x + angle.cos(),
                            0.25,
                            position.y + angle.sin(),
                        ),
                        EffectVelocity(Vec3::new(-angle.cos() * 1.8, 0.8, -angle.sin() * 1.8)),
                        EffectLifetime {
                            remaining: 0.42,
                            total: 0.42,
                        },
                    ));
                }
            }
            VisualEffect::Respawn { .. }
            | VisualEffect::LeaderChanged { .. }
            | VisualEffect::Victory { .. } => {
                commands.spawn((
                    Mesh3d(assets.ring.clone()),
                    MeshMaterial3d(assets.materials[palette].clone()),
                    ground_ring_transform(position),
                    EffectLifetime {
                        remaining: if matches!(event, VisualEffect::Victory { .. }) {
                            1.8
                        } else {
                            0.65
                        },
                        total: 0.65,
                    },
                ));
            }
        }
    }
}

fn ground_ring_transform(position: Vec2) -> Transform {
    Transform::from_xyz(position.x, 0.13, position.y)
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
}

fn capture_popup_label(percent: f32) -> Option<String> {
    (percent.is_finite() && percent >= 0.5).then(|| format!("+{percent:.1}%"))
}

#[allow(clippy::too_many_arguments)]
fn spawn_kill_effect(
    commands: &mut Commands,
    assets: &EffectAssets,
    tuning: &CameraTuning,
    settings: &PresentationSettings,
    camera_by_subject: &HashMap<Entity, Entity>,
    source: Entity,
    position: Vec2,
    color_id: u8,
    palette: usize,
    progress: KillProgress,
) {
    let profile = kill_effect_profile(progress);
    for index in 0..profile.particle_count {
        let angle = index as f32 * std::f32::consts::TAU / profile.particle_count as f32;
        let speed = profile.burst_speed * (0.82 + (index % 4) as f32 * 0.08);
        commands.spawn((
            Mesh3d(assets.fragment.clone()),
            MeshMaterial3d(assets.materials[palette].clone()),
            Transform::from_xyz(position.x, 0.72, position.y),
            EffectVelocity(Vec3::new(
                angle.cos() * speed,
                2.45 + (index % 5) as f32 * 0.12,
                angle.sin() * speed,
            )),
            EffectLifetime {
                remaining: profile.particle_lifetime,
                total: profile.particle_lifetime,
            },
        ));
    }
    commands.spawn((
        Mesh3d(assets.ring.clone()),
        MeshMaterial3d(assets.materials[palette].clone()),
        ground_ring_transform(position).with_scale(Vec3::splat(profile.ring_scale)),
        EffectLifetime {
            remaining: profile.ring_lifetime,
            total: profile.ring_lifetime,
        },
    ));
    commands.spawn((
        Text2d::new(kill_popup_label(progress)),
        TextFont {
            font: assets.popup_font.clone(),
            font_size: FontSize::Px(25.0 + profile.tier as f32 * 3.0),
            ..default()
        },
        TextColor(palette_color(color_id)),
        Text2dShadow {
            offset: Vec2::new(2.0, -2.0),
            color: Color::srgba(0.01, 0.015, 0.025, 0.94),
        },
        Transform::from_xyz(position.x, 2.15, position.y),
        RisingPopup,
        EffectLifetime {
            remaining: profile.popup_lifetime,
            total: profile.popup_lifetime,
        },
    ));
    if !settings.reduced_motion
        && let Some(&camera) = camera_by_subject.get(&source)
    {
        commands.entity(camera).insert(CameraFovPulse {
            remaining: profile.fov_pulse_duration,
            total: profile.fov_pulse_duration,
            amount_radians: tuning.kill_fov_pulse_radians * profile.fov_scale,
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct KillEffectProfile {
    tier: u8,
    particle_count: usize,
    burst_speed: f32,
    particle_lifetime: f32,
    ring_scale: f32,
    ring_lifetime: f32,
    popup_lifetime: f32,
    fov_pulse_duration: f32,
    fov_scale: f32,
}

fn kill_effect_profile(progress: KillProgress) -> KillEffectProfile {
    let tier = match progress.total {
        0..=1 => 0,
        2..=3 => 1,
        4..=6 => 2,
        _ => 3,
    };
    KillEffectProfile {
        tier,
        particle_count: 10 + tier as usize * 5 + progress.streak.min(4) as usize,
        burst_speed: 2.7 + tier as f32 * 0.45,
        particle_lifetime: 0.55 + tier as f32 * 0.06,
        ring_scale: 1.0 + tier as f32 * 0.18,
        ring_lifetime: 0.62 + tier as f32 * 0.12,
        popup_lifetime: 0.9 + tier as f32 * 0.08,
        fov_pulse_duration: 0.72 + tier as f32 * 0.05,
        fov_scale: 1.0 + tier as f32 * 0.12,
    }
}

fn kill_popup_label(progress: KillProgress) -> String {
    let title = match progress.total {
        0..=1 => "KILL",
        2..=3 => "HOT",
        4..=6 => "RAMPAGE",
        _ => "UNSTOPPABLE",
    };
    if progress.streak >= 2 {
        format!("{title} x{}", progress.streak)
    } else {
        format!("{title} +1")
    }
}

#[allow(clippy::type_complexity)]
fn update_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut effects: Query<(
        Entity,
        &mut EffectLifetime,
        &mut Transform,
        Option<&mut EffectVelocity>,
        Option<&RisingPopup>,
    )>,
) {
    let dt = time.delta_secs().min(0.05);
    for (entity, mut lifetime, mut transform, velocity, popup) in &mut effects {
        lifetime.remaining -= dt;
        if lifetime.remaining <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let progress = 1.0 - lifetime.remaining / lifetime.total.max(0.001);
        if let Some(mut velocity) = velocity {
            transform.translation += velocity.0 * dt;
            velocity.0.y -= 8.5 * dt;
            transform.rotate(Quat::from_rotation_x(dt * 5.0));
            transform.scale = Vec3::splat((1.0 - progress).max(0.01));
        } else if popup.is_some() {
            transform.translation.y += 0.7 * dt;
            transform.scale = Vec3::splat((1.0 - progress * progress).max(0.01));
        } else {
            transform.scale = Vec3::splat(1.0 + progress * 3.5);
        }
    }
}

fn apply_camera_shake(
    time: Res<Time>,
    settings: Res<crate::render::PresentationSettings>,
    mut cameras: Query<(&mut Transform, &mut CameraTrauma), With<PlayerCamera>>,
) {
    for (mut transform, mut trauma) in &mut cameras {
        trauma.amount = (trauma.amount - time.delta_secs() * 4.0).max(0.0);
        trauma.phase += time.delta_secs() * 47.0;
        if settings.reduced_motion || settings.camera_shake <= 0.0 {
            continue;
        }
        let strength = trauma.amount * trauma.amount * settings.camera_shake * 0.18;
        transform.translation.x += trauma.phase.sin() * strength;
        transform.translation.z += (trauma.phase * 1.37).sin() * strength;
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{Vec2, Vec3};

    use crate::match_game::KillProgress;

    use super::{
        capture_popup_label, ground_ring_transform, kill_effect_profile, kill_popup_label,
    };

    #[test]
    fn capture_popup_suppresses_unreadable_micro_captures() {
        assert_eq!(capture_popup_label(0.49), None);
        assert_eq!(capture_popup_label(f32::NAN), None);
        assert_eq!(capture_popup_label(2.36).as_deref(), Some("+2.4%"));
    }

    #[test]
    fn effect_rings_lie_flat_on_the_field() {
        let transform = ground_ring_transform(Vec2::new(2.0, -3.0));
        assert!((transform.rotation * Vec3::Z).abs_diff_eq(Vec3::Y, 1.0e-6));
        assert_eq!(transform.translation, Vec3::new(2.0, 0.13, -3.0));
    }

    #[test]
    fn kill_effects_escalate_without_unbounded_particle_growth() {
        let starter = kill_effect_profile(KillProgress {
            total: 1,
            streak: 1,
        });
        let rampage = kill_effect_profile(KillProgress {
            total: 5,
            streak: 4,
        });
        let unstoppable = kill_effect_profile(KillProgress {
            total: 100,
            streak: 100,
        });
        assert!(rampage.particle_count > starter.particle_count);
        assert!(unstoppable.particle_count <= 29);
        assert!(unstoppable.ring_scale > rampage.ring_scale);
    }

    #[test]
    fn kill_popup_exposes_streak_and_total_kill_tier() {
        assert_eq!(
            kill_popup_label(KillProgress {
                total: 1,
                streak: 1
            }),
            "KILL +1"
        );
        assert_eq!(
            kill_popup_label(KillProgress {
                total: 3,
                streak: 3
            }),
            "HOT x3"
        );
        assert_eq!(
            kill_popup_label(KillProgress {
                total: 7,
                streak: 1
            }),
            "UNSTOPPABLE +1"
        );
    }
}
