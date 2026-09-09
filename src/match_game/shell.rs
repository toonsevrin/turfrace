//! Application-shell adapter for the device-free simulation.
//!
//! This module is the only match-game code that knows about lobby setup,
//! application states, and input devices. [`SimulationPlugin`] remains usable
//! without it.

use bevy::prelude::*;

use crate::{
    app_state::AppState,
    config::GameConfig,
    input::HumanController,
    lobby::{MatchLaunchMode, MatchSetup},
};

use super::{
    Competitor, MatchPhase, MatchPurpose, MatchSession, MatchSpec, RosterDescriptor,
    SimulationPaused, SimulationPlugin, start_simulation,
};

pub struct MatchPlugin;

impl Plugin for MatchPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SimulationPlugin)
            .init_resource::<AttractSeedSequence>()
            .add_systems(OnEnter(AppState::MatchLoading), begin_from_lobby)
            .add_systems(
                OnEnter(AppState::Home),
                begin_attract_match.run_if(attract_match_is_missing),
            )
            .add_systems(
                OnEnter(AppState::Lobby),
                begin_attract_match.run_if(attract_match_is_missing),
            )
            .add_systems(
                Update,
                (
                    sync_pause,
                    sync_active_phase,
                    advance_result_hold,
                    observe_finished_match,
                )
                    .chain(),
            );
    }
}

/// Compatibility adapter for existing shell callers. New callers should
/// construct a [`MatchSpec`] and invoke [`start_simulation`] instead.
pub fn start_match(world: &mut World, setup: &MatchSetup) {
    start_match_for(world, setup, MatchPurpose::Playable);
}

pub(super) fn begin_from_lobby(world: &mut World) {
    let setup = world.resource::<MatchSetup>().clone();
    let mode = std::mem::take(&mut *world.resource_mut::<MatchLaunchMode>());
    start_match_for_with_countdown(
        world,
        &setup,
        MatchPurpose::Playable,
        (mode == MatchLaunchMode::LobbyCountdownCompleted).then_some(0),
    );
}

const ATTRACT_NPC_COUNT: u8 = 6;
const ATTRACT_SEED_START: u64 = 0xA77A_C7A5_5EED;
const SEED_STEP: u64 = 6_364_136_223_846_793_005;

#[derive(Resource, Debug, Clone, Copy)]
pub(super) struct AttractSeedSequence(pub u64);

impl Default for AttractSeedSequence {
    fn default() -> Self {
        Self(ATTRACT_SEED_START)
    }
}

pub(super) fn begin_attract_match(world: &mut World) {
    let seed = {
        let mut sequence = world.resource_mut::<AttractSeedSequence>();
        let seed = sequence.0;
        sequence.0 = seed.wrapping_mul(SEED_STEP).wrapping_add(1);
        seed
    };
    let config = world.resource::<GameConfig>().clone();
    let mut spec = MatchSpec::from_config(
        seed,
        (0..ATTRACT_NPC_COUNT)
            .map(|_| RosterDescriptor::Npc)
            .collect(),
        &config,
    );
    spec.npc_roster_seed = seed ^ 0x4e50_4352_4f53_5445;
    spec.countdown_ticks = 0;
    spec.purpose = MatchPurpose::Attract;
    spec.rules.victory_enabled = false;
    start_simulation(world, &spec);
}

fn sync_active_phase(
    session: Res<MatchSession>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
) {
    match (state.get(), session.phase) {
        (AppState::MatchLoading, MatchPhase::Countdown) => next.set(AppState::Countdown),
        (AppState::MatchLoading | AppState::Countdown, MatchPhase::Running) => {
            next.set(AppState::Playing)
        }
        _ => {}
    }
}

fn advance_result_hold(
    time: Res<Time>,
    paused: Res<SimulationPaused>,
    mut session: ResMut<MatchSession>,
) {
    if !paused.0 && session.phase == MatchPhase::Finished {
        session.result_hold_remaining =
            (session.result_hold_remaining - time.delta_secs()).max(0.0);
    }
}

fn sync_pause(state: Res<State<AppState>>, mut paused: ResMut<SimulationPaused>) {
    paused.0 = *state.get() == AppState::Paused;
}

/// Shell navigation observes the authoritative phase rather than being
/// mutated by a simulation system.
fn observe_finished_match(
    mut session: ResMut<MatchSession>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if session.phase != MatchPhase::Finished {
        return;
    }
    match state.get() {
        AppState::Playing => {
            // Result hold is presentation timing, initialized only when the
            // shell observes the authoritative Finished transition.
            if session.result_hold_remaining <= 0.0 {
                session.result_hold_remaining = 3.0;
            }
            next.set(AppState::GameOver);
        }
        AppState::GameOver if session.result_hold_remaining <= 0.0 => next.set(AppState::Results),
        _ => {}
    }
}

fn attract_match_is_missing(session: Res<MatchSession>) -> bool {
    session.purpose != MatchPurpose::Attract || session.phase == MatchPhase::Idle
}

pub(super) fn start_match_for(world: &mut World, setup: &MatchSetup, purpose: MatchPurpose) {
    start_match_for_with_countdown(world, setup, purpose, None);
}

fn start_match_for_with_countdown(
    world: &mut World,
    setup: &MatchSetup,
    purpose: MatchPurpose,
    countdown_override: Option<u64>,
) {
    let config = world.resource::<GameConfig>().clone();
    let count = usize::from(setup.total_competitors.clamp(2, 12));
    let count = count
        .max(setup.humans.len())
        .min(crate::ids::MAX_COMPETITORS);
    let mut roster = setup
        .humans
        .iter()
        .map(|human| RosterDescriptor::Human {
            identity: human.profile_id.clone().unwrap_or_default(),
            display_name: human.display_name.clone(),
            color_id: human.color_id,
            pattern_id: human.pattern_id,
        })
        .collect::<Vec<_>>();
    roster.resize(count, RosterDescriptor::Npc);
    roster.truncate(crate::ids::MAX_COMPETITORS);
    let mut spec = MatchSpec::from_config(setup.field_seed, roster, &config);
    spec.npc_roster_seed = setup.npc_roster_seed;
    spec.npc_difficulty = setup.npc_difficulty;
    spec.purpose = purpose;
    if purpose == MatchPurpose::Attract {
        spec.countdown_ticks = 0;
        spec.rules.victory_enabled = false;
    }
    if let Some(countdown_ticks) = countdown_override {
        spec.countdown_ticks = countdown_ticks;
    }
    start_simulation(world, &spec);
    attach_human_devices(world, &setup.humans);
}

fn attach_human_devices(world: &mut World, humans: &[crate::lobby::HumanSetup]) {
    let entities = {
        let mut query = world.query::<(Entity, &Competitor)>();
        query
            .iter(world)
            .filter_map(|(entity, competitor)| {
                (competitor.kind == super::CompetitorKind::Human)
                    .then_some((entity, usize::from(competitor.id.0)))
            })
            .collect::<Vec<_>>()
    };
    for (entity, roster_index) in entities {
        if let Some(human) = humans.get(roster_index) {
            world.entity_mut(entity).insert(HumanController {
                device: human.device,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_assignment_uses_roster_ids_not_query_order() {
        let mut world = World::new();
        let entities = [1, 0].map(|id| {
            world
                .spawn(Competitor {
                    id: crate::ids::CompetitorId(id),
                    display_name: id.to_string(),
                    kind: super::super::CompetitorKind::Human,
                    color_id: id,
                    pattern_id: 0,
                })
                .id()
        });
        let humans = [
            crate::input::InputDeviceId::KeyboardPrimary,
            crate::input::InputDeviceId::Mouse,
        ]
        .map(|device| crate::lobby::HumanSetup {
            device,
            profile_id: None,
            display_name: String::new(),
            color_id: 0,
            pattern_id: 0,
        });
        attach_human_devices(&mut world, &humans);
        assert_eq!(
            world.get::<HumanController>(entities[0]).unwrap().device,
            humans[1].device
        );
        assert_eq!(
            world.get::<HumanController>(entities[1]).unwrap().device,
            humans[0].device
        );
    }

    #[test]
    fn navigation_waits_for_authoritative_phase_not_frame_count() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .insert_state(AppState::MatchLoading)
            .insert_resource(MatchSession {
                phase: MatchPhase::Loading,
                ..default()
            })
            .add_systems(Update, sync_active_phase);
        for _ in 0..12 {
            app.update();
        }
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::MatchLoading
        );
        app.world_mut().resource_mut::<MatchSession>().phase = MatchPhase::Countdown;
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Countdown
        );
        app.world_mut().resource_mut::<MatchSession>().phase = MatchPhase::Running;
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Playing
        );
    }

    #[test]
    fn generation_acknowledgement_is_reset_for_every_start() {
        let mut world = World::new();
        world.insert_resource(GameConfig::default());
        world.insert_resource(super::super::PresentationReady(Some(99)));
        let setup = MatchSetup::default();
        start_match(&mut world, &setup);
        // Starting a real shell match must discard a previous acknowledgement.
        assert_eq!(world.resource::<super::super::PresentationReady>().0, None);
    }
}
