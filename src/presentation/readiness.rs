//! Match-scoped presentation readiness.
//!
//! Readiness is a generation-tagged handshake. The main world verifies the
//! expected scene, while the render world verifies that the current extraction
//! has produced visible, ready pipeline witnesses for every active camera.

use std::sync::{Arc, Mutex};

use bevy::{
    pbr::SpecializedMaterialPipelineCache,
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        render_resource::{CachedPipelineState, PipelineCache},
        sync_world::MainEntity,
        view::{ExtractedView, RenderVisibleEntities, RetainedViewEntity},
    },
};

use crate::{
    camera::{PlayerCamera, SpectatorCamera, ViewportSubject},
    match_game::{Competitor, MatchGeneration, PresentationReady},
    render::{
        CompetitorProxy, CompetitorVisual, FieldVisual, FlatMaterial, PaperMaterial, RenderAssets,
        TerritoryMaterial, TerritoryVisual, TrailPipelineWarmup,
    },
};

/// Pipeline variants that must be witnessed in each relevant camera view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MaterialFamily {
    FlatOpaqueBack,
    FlatOpaqueFront,
    FlatTransparent,
    Paper,
    Territory,
}

const REQUIRED_FAMILIES: [MaterialFamily; 5] = [
    MaterialFamily::FlatOpaqueBack,
    MaterialFamily::FlatOpaqueFront,
    MaterialFamily::FlatTransparent,
    MaterialFamily::Paper,
    MaterialFamily::Territory,
];

#[derive(Clone, Copy)]
struct PipelineWitness {
    entity: MainEntity,
    family: MaterialFamily,
}

/// The render world cannot directly mutate the app-world acknowledgement, so
/// this small bridge carries only a generation-tagged result between worlds.
#[derive(Resource, Clone)]
struct RenderReadinessBridge(Arc<Mutex<Option<u64>>>);

#[derive(Resource, Default)]
struct RenderReadinessSnapshot {
    generation: Option<u64>,
    /// Mesh children of proxies whose source and generation are current.
    proxy_mesh_entities: Vec<MainEntity>,
    witnesses: Vec<PipelineWitness>,
    active_views: Vec<RetainedViewEntity>,
}

pub(super) fn install(app: &mut App) {
    // Headless apps use the explicit acknowledgement in the core simulation
    // interface. There is no render-world inference in that mode.
    if app.get_sub_app(RenderApp).is_none() {
        return;
    }

    let bridge = RenderReadinessBridge(Arc::new(Mutex::new(None)));
    app.insert_resource(bridge.clone());
    app.add_systems(PostUpdate, evaluate_main_world_readiness);

    let render_app = app
        .get_sub_app_mut(RenderApp)
        .expect("RenderApp disappeared while installing readiness");
    render_app
        .insert_resource(bridge)
        .init_resource::<RenderReadinessSnapshot>()
        .add_systems(ExtractSchedule, extract_match_readiness)
        // PipelineCache processes queued pipelines inside RenderSystems::Render.
        // Observing after that set avoids acknowledging an empty pre-extraction
        // queue or a pipeline that is still queued.
        .add_systems(
            Render,
            observe_render_readiness.after(RenderSystems::Render),
        );
}

#[allow(clippy::too_many_arguments)]
fn extract_match_readiness(
    mut commands: Commands,
    generation: Extract<Option<Res<MatchGeneration>>>,
    competitors: Extract<Query<Entity, With<Competitor>>>,
    proxies: Extract<Query<(Entity, &CompetitorProxy, &Children)>>,
    cameras: Extract<Query<(Entity, &Camera), With<Camera3d>>>,
    flat_meshes: Extract<Query<(Entity, &MeshMaterial3d<FlatMaterial>)>>,
    territory_meshes: Extract<Query<(Entity, &MeshMaterial3d<TerritoryMaterial>)>>,
    paper_meshes: Extract<Query<(Entity, &MeshMaterial3d<PaperMaterial>)>>,
    warmups: Extract<Query<(Entity, &TrailPipelineWarmup)>>,
    flat_assets: Extract<Option<Res<Assets<FlatMaterial>>>>,
) {
    let generation = generation.as_ref().map(|generation| generation.0);

    let current_competitors: Vec<_> = competitors.iter().collect();
    let current_proxy_children: Vec<_> = proxies
        .iter()
        .filter(|(_, proxy, _)| {
            generation.is_some_and(|generation| {
                proxy.generation == generation && current_competitors.contains(&proxy.source)
            })
        })
        .flat_map(|(_, _, children)| children.iter())
        .collect();
    let current_warmups: Vec<_> = warmups
        .iter()
        .filter(|(_, warmup)| generation == Some(warmup.generation))
        .map(|(entity, _)| entity)
        .collect();

    let mut witnesses = Vec::new();
    let mut proxy_mesh_entities = Vec::new();
    if let Some(flat_assets) = flat_assets.as_deref() {
        for (entity, material_handle) in &flat_meshes {
            let Some(material) = flat_assets.get(&material_handle.0) else {
                continue;
            };
            let family = if !matches!(material.alpha_mode, AlphaMode::Opaque) {
                // Transparent flat meshes from old trails must not satisfy the
                // witness. Only the current loading warmup is intentional.
                if !current_warmups.contains(&entity) {
                    continue;
                }
                MaterialFamily::FlatTransparent
            } else if material.cull_front {
                MaterialFamily::FlatOpaqueFront
            } else {
                MaterialFamily::FlatOpaqueBack
            };

            if family != MaterialFamily::FlatTransparent
                && !current_proxy_children.contains(&entity)
            {
                continue;
            }
            let entity = MainEntity::from(entity);
            witnesses.push(PipelineWitness { entity, family });
            if current_proxy_children.contains(&entity.id()) {
                proxy_mesh_entities.push(entity);
            }
        }
    }
    witnesses.extend(territory_meshes.iter().map(|(entity, _)| PipelineWitness {
        entity: MainEntity::from(entity),
        family: MaterialFamily::Territory,
    }));
    witnesses.extend(paper_meshes.iter().map(|(entity, _)| PipelineWitness {
        entity: MainEntity::from(entity),
        family: MaterialFamily::Paper,
    }));

    let active_views = cameras
        .iter()
        .filter(|(_, camera)| camera.is_active && valid_viewport(camera.viewport.as_ref()))
        .map(|(entity, _)| RetainedViewEntity::new(MainEntity::from(entity), None, 0))
        .collect();

    commands.insert_resource(RenderReadinessSnapshot {
        generation,
        proxy_mesh_entities,
        witnesses,
        active_views,
    });
}

#[allow(clippy::too_many_arguments)]
fn observe_render_readiness(
    snapshot: Res<RenderReadinessSnapshot>,
    pipelines: Option<Res<PipelineCache>>,
    specialized: Option<Res<SpecializedMaterialPipelineCache>>,
    views: Query<(&ExtractedView, &RenderVisibleEntities)>,
    bridge: Res<RenderReadinessBridge>,
) {
    let Some(generation) = snapshot.generation else {
        return;
    };
    if snapshot.proxy_mesh_entities.is_empty()
        || snapshot.witnesses.is_empty()
        || snapshot.active_views.is_empty()
    {
        return;
    }

    let Some((pipelines, specialized)) = pipelines.as_deref().zip(specialized.as_deref()) else {
        return;
    };
    let prepared = snapshot.active_views.iter().all(|expected_view| {
        let Some((_, visible)) = views
            .iter()
            .find(|(view, _)| view.retained_view_entity == *expected_view)
        else {
            return false;
        };
        let Some(view_cache) = specialized.get(expected_view) else {
            return false;
        };

        // A mesh that is outside a camera's frustum is not a useful witness for
        // that camera. Consider only meshes actually visible in this view;
        // this prevents legitimate culling from turning readiness into a leak.
        let mut pipeline_ready = |entity: MainEntity| {
            visible_in_view(visible, entity)
                && view_cache.get(&entity).is_some_and(|pipeline| {
                    let state = pipelines.get_render_pipeline_state(*pipeline);
                    pipeline_state_is_ready(state)
                })
        };
        snapshot
            .proxy_mesh_entities
            .iter()
            .copied()
            .any(&mut pipeline_ready)
            && required_families_ready(|family| {
                snapshot
                    .witnesses
                    .iter()
                    .any(|witness| witness.family == family && pipeline_ready(witness.entity))
            })
    });
    if prepared {
        // Keep this acknowledgement until the main world observes it. Clearing
        // it on extraction races with pipelined rendering and loses the signal.
        set_bridge(&bridge.0, Some(generation));
    }
}

fn visible_in_view(visible: &RenderVisibleEntities, entity: MainEntity) -> bool {
    visible.classes.values().any(|class| {
        class
            .entities_cpu_culling
            .iter()
            .any(|(_, visible_entity)| *visible_entity == entity)
            || class.entities_gpu_culling.contains_key(&entity)
    })
}

fn required_families_ready(mut witness_ready: impl FnMut(MaterialFamily) -> bool) -> bool {
    REQUIRED_FAMILIES.iter().copied().all(&mut witness_ready)
}

#[allow(clippy::too_many_arguments)]
fn evaluate_main_world_readiness(
    generation: Option<Res<MatchGeneration>>,
    presentation_ready: Option<ResMut<PresentationReady>>,
    field: Option<Res<FieldVisual>>,
    territory: Option<Res<TerritoryVisual>>,
    render_assets: Option<Res<RenderAssets>>,
    meshes: Option<Res<Assets<Mesh>>>,
    flat_materials: Option<Res<Assets<FlatMaterial>>>,
    paper_materials: Option<Res<Assets<PaperMaterial>>>,
    competitors: Query<(
        Entity,
        &Competitor,
        Option<&CompetitorVisual>,
        Option<&ViewportSubject>,
    )>,
    proxies: Query<&CompetitorProxy>,
    player_cameras: Query<(&PlayerCamera, &Camera)>,
    spectator_cameras: Query<(&SpectatorCamera, &Camera)>,
    bridge: Res<RenderReadinessBridge>,
) {
    let (Some(generation), Some(mut presentation_ready)) = (generation, presentation_ready) else {
        return;
    };
    // Readiness is a one-time generation barrier. Do not revoke it when the
    // loading-only pipeline witnesses are removed or a proxy later respawns.
    if presentation_ready.0 == Some(generation.0) {
        return;
    }
    let visual_resources_ready = field
        .as_deref()
        .is_some_and(|field| field.revision != 0 && field.contour.len() >= 3)
        && territory
            .as_deref()
            .is_some_and(|territory| territory.revision != 0 && territory.is_valid())
        && render_assets
            .as_deref()
            .zip(meshes.as_deref())
            .zip(flat_materials.as_deref().zip(paper_materials.as_deref()))
            .is_some_and(|((assets, meshes), (flat, papers))| {
                assets.is_ready(meshes, flat, papers)
            });

    let mut visual_count = 0;
    let mut human_entities = Vec::new();
    let mut human_subject_count = 0;
    let mut competitor_entities = Vec::new();
    for (entity, competitor, visual, subject) in &competitors {
        competitor_entities.push(entity);
        visual_count += usize::from(visual.is_some());
        if competitor.kind == crate::match_game::CompetitorKind::Human {
            human_entities.push(entity);
            human_subject_count += usize::from(subject.is_some());
        }
    }
    let all_proxies_current = proxies.iter().all(|proxy| {
        proxy.generation == generation.0 && competitor_entities.contains(&proxy.source)
    });
    let current_proxy_count = proxies
        .iter()
        .filter(|proxy| {
            proxy.generation == generation.0 && competitor_entities.contains(&proxy.source)
        })
        .count();
    let every_competitor_has_proxy = competitor_entities.iter().all(|source| {
        proxies
            .iter()
            .any(|proxy| proxy.generation == generation.0 && proxy.source == *source)
    });
    let scene_ready = !competitor_entities.is_empty()
        && visual_count == competitor_entities.len()
        && all_proxies_current
        && every_competitor_has_proxy
        && current_proxy_count == competitor_entities.len()
        && cameras_ready(
            human_entities.len(),
            human_subject_count,
            &human_entities,
            &player_cameras,
            &spectator_cameras,
        );

    let renderer_generation = bridge_generation(&bridge.0);
    presentation_ready.0 = readiness_for_generation(
        Some(generation.0),
        renderer_generation,
        visual_resources_ready && scene_ready,
    );
}

fn cameras_ready(
    human_count: usize,
    subject_count: usize,
    human_entities: &[Entity],
    player_cameras: &Query<(&PlayerCamera, &Camera)>,
    spectator_cameras: &Query<(&SpectatorCamera, &Camera)>,
) -> bool {
    if human_count > 0 {
        let mut slots: Vec<_> = player_cameras
            .iter()
            .map(|(camera, _)| camera.slot)
            .collect();
        slots.sort_unstable();
        let expected_slots: Vec<_> = (0..human_count as u8).collect();
        subject_count == human_count
            && spectator_cameras.is_empty()
            && slots == expected_slots
            && player_cameras.iter().count() == human_count
            && player_cameras.iter().all(|(camera, view)| {
                human_entities.contains(&camera.subject)
                    && view.is_active
                    && valid_viewport(view.viewport.as_ref())
            })
    } else {
        player_cameras.is_empty()
            && spectator_cameras.iter().count() == 1
            && spectator_cameras
                .iter()
                .all(|(_, camera)| camera.is_active && valid_viewport(camera.viewport.as_ref()))
    }
}

fn valid_viewport(viewport: Option<&bevy::camera::Viewport>) -> bool {
    viewport.is_some_and(|viewport| viewport.physical_size.x > 0 && viewport.physical_size.y > 0)
}

fn pipeline_state_is_ready(state: &CachedPipelineState) -> bool {
    matches!(state, CachedPipelineState::Ok(_))
}

fn readiness_for_generation(
    current_generation: Option<u64>,
    renderer_generation: Option<u64>,
    scene_ready: bool,
) -> Option<u64> {
    current_generation.filter(|generation| scene_ready && renderer_generation == Some(*generation))
}

fn set_bridge(bridge: &Mutex<Option<u64>>, generation: Option<u64>) {
    *bridge
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = generation;
}

fn bridge_generation(bridge: &Mutex<Option<u64>>) -> Option<u64> {
    *bridge
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use bevy::shader::ShaderCacheError;

    use super::*;

    #[test]
    fn acknowledged_generation_survives_warmup_cleanup_but_not_restart() {
        let mut app = App::new();
        app.insert_resource(MatchGeneration(7))
            .insert_resource(PresentationReady(Some(7)))
            .insert_resource(RenderReadinessBridge(Arc::new(Mutex::new(Some(7)))))
            .add_systems(Update, evaluate_main_world_readiness);
        app.update();
        assert_eq!(app.world().resource::<PresentationReady>().0, Some(7));
        app.world_mut().resource_mut::<MatchGeneration>().0 = 8;
        app.update();
        assert_eq!(app.world().resource::<PresentationReady>().0, None);
    }

    #[test]
    fn stale_renderer_ack_does_not_ready_a_new_match() {
        assert_eq!(readiness_for_generation(Some(8), Some(7), true), None);
        assert_eq!(readiness_for_generation(Some(8), Some(8), true), Some(8));
    }

    #[test]
    fn missing_scene_or_witness_keeps_the_match_unready() {
        assert_eq!(readiness_for_generation(Some(3), Some(3), false), None);
        assert!(!required_families_ready(|family| {
            family != MaterialFamily::FlatTransparent
        }));
        assert!(required_families_ready(|_| true));
    }

    #[test]
    fn queued_and_error_pipelines_are_not_prepared() {
        assert!(!pipeline_state_is_ready(&CachedPipelineState::Queued));
        assert!(!pipeline_state_is_ready(&CachedPipelineState::Err(
            ShaderCacheError::ShaderImportNotYetAvailable,
        )));
    }

    #[test]
    fn invalid_viewports_are_not_camera_witnesses() {
        assert!(!valid_viewport(None));
        assert!(!valid_viewport(Some(&bevy::camera::Viewport {
            physical_position: UVec2::ZERO,
            physical_size: UVec2::new(1, 0),
            depth: 0.0..1.0,
        })));
    }
}
