use bevy::{prelude::*, time::Fixed};

use super::{
    Competitor, CompetitorKind, EliminationFeed, LifeState, LifeStatus, MatchGeneration,
    MatchPhase, MatchRules, MatchSession, MatchSpec, MatchStatistics, PresentationReady,
    RosterDescriptor, SimulationEvent, SimulationEvents, SimulationPlugin,
    outcomes::{EliminationOutcome, EliminationQuery, EliminationResources},
    start_simulation,
};
use crate::{
    board::BoardGrid,
    config::GameConfig,
    ids::CompetitorId,
    movement::CompetitorMotion,
    npc::{NpcEvent, NpcEventQueue},
    territory_map::TerritoryMap,
    trail::{ActiveTrail, update_trail_raster},
};

#[derive(Resource, Default)]
struct TestOutcomes(Vec<EliminationOutcome>);

fn resolve_test_outcomes(
    mut commands: Commands,
    mut board: ResMut<BoardGrid>,
    mut territory: ResMut<TerritoryMap>,
    rules: Res<MatchRules>,
    mut effects: EliminationResources,
    mut outcomes: ResMut<TestOutcomes>,
    mut query: Query<EliminationQuery>,
) {
    let outcomes = std::mem::take(&mut outcomes.0);
    super::outcomes::resolve_eliminations(
        outcomes,
        &mut commands,
        &mut board,
        &mut territory,
        &rules,
        &mut effects,
        &mut query,
    );
}

fn resolver_app(outcomes: Vec<EliminationOutcome>, rules: MatchRules) -> App {
    let config = GameConfig::default();
    let mut board = BoardGrid::generate(0x0E11_A1A7, 3, &config);
    let mut territory = TerritoryMap::from_board(&board);
    let positions = [Vec2::new(-12.0, 0.0), Vec2::ZERO, Vec2::new(12.0, 0.0)];
    for (index, position) in positions.into_iter().enumerate() {
        territory.seed_owner(position, 2.0, CompetitorId(index as u8));
    }

    let mut app = App::new();
    app.insert_resource(board.clone())
        .insert_resource(territory)
        .insert_resource(config)
        .insert_resource(rules)
        .insert_resource(MatchSession {
            phase: MatchPhase::Running,
            elapsed_seconds: 12.5,
            ..default()
        })
        .insert_resource(SimulationEvents::default())
        .insert_resource(EliminationFeed::default())
        .insert_resource(NpcEventQueue::default())
        .insert_resource(TestOutcomes(outcomes))
        .add_systems(Update, resolve_test_outcomes);

    for (index, position) in positions.into_iter().enumerate() {
        let id = CompetitorId(index as u8);
        let anchor = board.world_to_cell(position).expect("fixture is on board");
        let mut trail = ActiveTrail::new(id, anchor, position, Vec2::Y);
        trail.append_exact(position + Vec2::Y * 2.0);
        update_trail_raster(&mut board, &mut trail, 0.4);
        app.world_mut().spawn((
            Competitor {
                id,
                display_name: format!("Player {index}"),
                kind: CompetitorKind::Npc,
                color_id: index as u8,
                pattern_id: index as u8,
            },
            CompetitorMotion::new(position, Vec2::Y),
            LifeState::alive(),
            MatchStatistics::default(),
            trail,
        ));
    }
    *app.world_mut().resource_mut::<BoardGrid>() = board;
    app
}

fn entity_for(world: &mut World, id: CompetitorId) -> Entity {
    world
        .query::<(Entity, &Competitor)>()
        .iter(world)
        .find(|(_, competitor)| competitor.id == id)
        .map(|(entity, _)| entity)
        .expect("fixture competitor exists")
}

fn statistics(world: &World, id: CompetitorId) -> MatchStatistics {
    world
        .iter_entities()
        .find_map(|entity| {
            let competitor = entity.get::<Competitor>()?;
            let stats = entity.get::<MatchStatistics>()?;
            (competitor.id == id).then_some(*stats)
        })
        .expect("fixture statistics exist")
}

#[test]
fn resolver_applies_each_cause_once_and_updates_all_effect_channels() {
    let mut app = resolver_app(
        vec![
            EliminationOutcome::trail_collision(CompetitorId(0), None),
            EliminationOutcome::trail_collision(CompetitorId(1), Some(CompetitorId(2))),
        ],
        MatchRules::default(),
    );
    app.update();

    for id in [CompetitorId(0), CompetitorId(1)] {
        let entity = entity_for(app.world_mut(), id);
        let world = app.world();
        assert_eq!(
            world.get::<LifeState>(entity).unwrap().status,
            LifeStatus::Respawning
        );
        assert!(world.get::<ActiveTrail>(entity).is_none());
        assert!(world.resource::<TerritoryMap>().area(id) <= f32::EPSILON);
    }
    assert_eq!(statistics(app.world(), CompetitorId(2)).kills, 1);
    assert_eq!(statistics(app.world(), CompetitorId(0)).kills, 0);
    assert!(
        app.world()
            .resource::<BoardGrid>()
            .active_trail_bits
            .iter()
            .all(|bits| bits & 0b11 == 0)
    );

    let events = &app.world().resource::<SimulationEvents>().0;
    assert!(events.iter().any(|event| matches!(
        event,
        SimulationEvent::Death {
            victim: CompetitorId(0),
            killer: None,
            cause: super::DeathCause::SelfTrail
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        SimulationEvent::Death {
            victim: CompetitorId(1),
            killer: Some(CompetitorId(2)),
            cause: super::DeathCause::TrailCut
        }
    )));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, SimulationEvent::Death { .. }))
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, SimulationEvent::Kill { .. }))
            .count(),
        1
    );

    let feed = &app.world().resource::<EliminationFeed>().0;
    assert_eq!(feed.len(), 2);
    assert!(
        feed.iter()
            .any(|record| record.cause == super::DeathCause::SelfTrail)
    );
    assert!(
        feed.iter()
            .any(|record| record.cause == super::DeathCause::TrailCut)
    );
    assert!(
        app.world()
            .resource::<NpcEventQueue>()
            .0
            .iter()
            .any(|message| matches!(message.event, NpcEvent::Died { killer: None }))
    );
    assert!(
        app.world()
            .resource::<NpcEventQueue>()
            .0
            .iter()
            .any(|message| matches!(
                message.event,
                NpcEvent::CreditedKill {
                    victim: CompetitorId(1)
                }
            ))
    );
}

#[test]
fn duplicate_and_reciprocal_same_tick_deaths_are_idempotent_but_both_get_credit() {
    let mut app = resolver_app(
        vec![
            EliminationOutcome::trail_collision(CompetitorId(0), Some(CompetitorId(1))),
            EliminationOutcome::trail_collision(CompetitorId(0), Some(CompetitorId(1))),
            EliminationOutcome::trail_collision(CompetitorId(1), Some(CompetitorId(0))),
            EliminationOutcome::trail_collision(CompetitorId(1), Some(CompetitorId(0))),
        ],
        MatchRules::default(),
    );
    app.update();

    for id in [CompetitorId(0), CompetitorId(1)] {
        assert_eq!(statistics(app.world(), id).deaths, 1);
        assert_eq!(statistics(app.world(), id).kills, 1);
    }
    let events = &app.world().resource::<SimulationEvents>().0;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, SimulationEvent::Death { .. }))
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, SimulationEvent::Kill { .. }))
            .count(),
        2
    );
    assert_eq!(app.world().resource::<EliminationFeed>().0.len(), 2);
}

#[test]
fn scoring_disabled_keeps_death_effects_but_suppresses_kill_effects() {
    let rules = MatchRules {
        scoring_enabled: false,
        ..default()
    };
    let mut app = resolver_app(
        vec![EliminationOutcome::trail_collision(
            CompetitorId(0),
            Some(CompetitorId(1)),
        )],
        rules,
    );
    app.update();

    assert_eq!(statistics(app.world(), CompetitorId(0)).deaths, 1);
    assert_eq!(statistics(app.world(), CompetitorId(1)).kills, 0);
    let events = &app.world().resource::<SimulationEvents>().0;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Death { .. }))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Kill { .. }))
    );
    assert_eq!(app.world().resource::<EliminationFeed>().0.len(), 1);
}

#[test]
fn self_killer_is_not_credited_and_death_count_saturates() {
    let mut app = resolver_app(
        vec![EliminationOutcome::trail_collision(
            CompetitorId(0),
            Some(CompetitorId(0)),
        )],
        MatchRules::default(),
    );
    let entity = entity_for(app.world_mut(), CompetitorId(0));
    app.world_mut()
        .get_mut::<MatchStatistics>(entity)
        .unwrap()
        .deaths = u32::MAX;
    app.update();

    assert_eq!(statistics(app.world(), CompetitorId(0)).deaths, u32::MAX);
    assert_eq!(statistics(app.world(), CompetitorId(0)).kills, 0);
    assert!(
        app.world()
            .resource::<SimulationEvents>()
            .0
            .iter()
            .any(|event| matches!(
                event,
                SimulationEvent::Death {
                    victim: CompetitorId(0),
                    killer: Some(CompetitorId(0)),
                    ..
                }
            ))
    );
}

#[test]
fn respawn_disabled_keeps_eliminated_players_out_of_the_match() {
    let rules = MatchRules {
        respawn_enabled: false,
        victory_enabled: false,
        ..default()
    };
    let mut app = resolver_app(
        vec![EliminationOutcome::trail_collision(CompetitorId(0), None)],
        rules,
    );
    app.add_plugins(SimulationPlugin);
    app.world_mut().resource_mut::<PresentationReady>().0 = Some(0);
    app.update();

    let entity = entity_for(app.world_mut(), CompetitorId(0));
    assert_eq!(
        app.world().get::<LifeState>(entity).unwrap().status,
        LifeStatus::Eliminated
    );
    for _ in 0..8 {
        let dt = app.world().resource::<Time<Fixed>>().timestep();
        app.world_mut().resource_mut::<Time<Fixed>>().advance_by(dt);
        app.world_mut().run_schedule(FixedUpdate);
    }

    assert_eq!(
        app.world().get::<LifeState>(entity).unwrap().status,
        LifeStatus::Eliminated
    );
    assert!(
        !app.world()
            .resource::<SimulationEvents>()
            .0
            .iter()
            .any(|event| matches!(
                event,
                SimulationEvent::Respawn {
                    player: CompetitorId(0)
                }
            ))
    );
}

#[test]
fn victory_disabled_never_assigns_a_winner_even_when_area_is_full() {
    let config = GameConfig::default();
    let mut spec = MatchSpec::from_config(
        0xF00D,
        vec![RosterDescriptor::Npc, RosterDescriptor::Npc],
        &config,
    );
    spec.countdown_ticks = 0;
    spec.rules.victory_enabled = false;
    let mut app = App::new();
    app.insert_resource(config).add_plugins(SimulationPlugin);
    app.update();
    start_simulation(app.world_mut(), &spec);
    let generation = app.world().resource::<MatchGeneration>().0;
    let arena = app.world().resource::<TerritoryMap>().arena().clone();
    app.world_mut()
        .resource_mut::<TerritoryMap>()
        .apply_claim(CompetitorId(0), arena);
    app.world_mut().resource_mut::<PresentationReady>().0 = Some(generation);
    let dt = app.world().resource::<Time<Fixed>>().timestep();
    app.world_mut().resource_mut::<Time<Fixed>>().advance_by(dt);
    app.world_mut().run_schedule(FixedUpdate);

    assert_eq!(
        app.world().resource::<MatchSession>().phase,
        MatchPhase::Running
    );
    assert_eq!(app.world().resource::<MatchSession>().winner, None);
    assert!(
        !app.world()
            .resource::<SimulationEvents>()
            .0
            .iter()
            .any(|event| matches!(event, SimulationEvent::Victory { .. }))
    );
}

#[test]
fn collision_outcome_classifies_self_and_killer_deaths() {
    let victim = CompetitorId(2);
    assert_eq!(
        EliminationOutcome::trail_collision(victim, None).cause,
        super::DeathCause::SelfTrail
    );
    assert_eq!(
        EliminationOutcome::trail_collision(victim, Some(CompetitorId(1))).cause,
        super::DeathCause::TrailCut
    );
}

#[test]
fn displacement_outcome_always_has_a_killer() {
    let outcome = EliminationOutcome::displaced(CompetitorId(2), CompetitorId(1));
    assert_eq!(outcome.killer, Some(CompetitorId(1)));
    assert_eq!(outcome.cause, super::DeathCause::Displaced);
}
