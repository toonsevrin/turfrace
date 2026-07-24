use super::*;
use crate::{
    lobby::MatchSetup,
    match_game::lifecycle::{start_match, start_match_for},
};
use std::time::Duration;

#[test]
fn simulation_only_runs_in_the_shell_state_that_owns_the_match() {
    let playable = MatchSession {
        purpose: MatchPurpose::Playable,
        phase: MatchPhase::Running,
        ..default()
    };
    let attract = MatchSession {
        purpose: MatchPurpose::Attract,
        phase: MatchPhase::Running,
        ..default()
    };

    assert!(simulation_is_active(&AppState::Playing, &playable));
    assert!(simulation_is_active(&AppState::Home, &attract));
    assert!(!simulation_is_active(&AppState::Paused, &playable));
    assert!(!simulation_is_active(&AppState::Home, &playable));
    assert!(!simulation_is_active(&AppState::Playing, &attract));
}

#[test]
fn victory_uses_exact_area_threshold_and_allows_remaining_territory() {
    let mut app = App::new();
    let board = BoardGrid::generate(1, 2, &GameConfig::default());
    let mut territory = TerritoryMap::from_board(&board);
    let arena = territory.arena.clone();
    app.init_resource::<NextState<AppState>>()
        .insert_resource(GameConfig::default())
        .insert_resource(board)
        .insert_resource(territory.clone())
        .init_resource::<MatchSession>()
        .init_resource::<SimulationEvents>()
        .add_systems(Update, check_victory);

    let (min, max) = territory.arena.bounds().expect("generated arena bounds");
    let right_strip = |fraction: f32| {
        let cut_x = min.world().x + (max.world().x - min.world().x) * fraction;
        crate::geometry::MultiPolygon::from_outer(&[
            Vec2::new(cut_x, min.world().y - 1.0),
            Vec2::new(max.world().x + 1.0, min.world().y - 1.0),
            Vec2::new(max.world().x + 1.0, max.world().y + 1.0),
            Vec2::new(cut_x, max.world().y + 1.0),
        ])
        .intersection(&arena)
    };

    territory.territories[0] = territory.arena.difference(&right_strip(0.25));
    *app.world_mut().resource_mut::<TerritoryMap>() = territory.clone();
    app.update();
    assert_eq!(app.world().resource::<MatchSession>().winner, None);

    let remaining = right_strip(0.95);
    territory.territories[0] = territory.arena.difference(&remaining);
    territory.territories[1] = remaining;
    *app.world_mut().resource_mut::<TerritoryMap>() = territory;
    app.update();
    assert_eq!(
        app.world().resource::<MatchSession>().winner,
        Some(CompetitorId(0))
    );
}

#[test]
fn victory_emits_once_when_other_territory_remains() {
    let mut app = App::new();
    let board = BoardGrid::generate(1, 2, &GameConfig::default());
    let mut territory = TerritoryMap::from_board(&board);
    let (min, max) = territory.arena.bounds().expect("generated arena bounds");
    let cut_x = min.world().x + (max.world().x - min.world().x) * 0.95;
    let remaining = crate::geometry::MultiPolygon::from_outer(&[
        Vec2::new(cut_x, min.world().y - 1.0),
        Vec2::new(max.world().x + 1.0, min.world().y - 1.0),
        Vec2::new(max.world().x + 1.0, max.world().y + 1.0),
        Vec2::new(cut_x, max.world().y + 1.0),
    ])
    .intersection(&territory.arena);
    territory.territories[0] = territory.arena.difference(&remaining);
    territory.territories[1] = remaining;
    app.init_resource::<NextState<AppState>>()
        .insert_resource(GameConfig::default())
        .insert_resource(board)
        .insert_resource(territory)
        .init_resource::<MatchSession>()
        .init_resource::<SimulationEvents>()
        .add_systems(Update, check_victory);
    app.update();
    app.update();
    assert_eq!(
        app.world().resource::<SimulationEvents>().0.len(),
        1,
        "a finished match must not emit duplicate victory events"
    );
}

#[test]
fn elimination_feed_is_bounded_and_keeps_the_newest_records() {
    let mut feed = EliminationFeed::default();
    for index in 0..(EliminationFeed::CAPACITY + 2) {
        feed.push(EliminationRecord {
            victim: CompetitorId(index as u8),
            killer: None,
            cause: DeathCause::SelfTrail,
            match_time: index as f32,
        });
    }
    assert_eq!(feed.0.len(), EliminationFeed::CAPACITY);
    assert_eq!(
        feed.0.first().map(|entry| entry.victim),
        Some(CompetitorId(2))
    );
    assert_eq!(
        feed.0.last().map(|entry| entry.victim),
        Some(CompetitorId(6))
    );
}

#[test]
fn kill_streak_grows_with_kills_and_resets_on_death() {
    let mut stats = MatchStatistics::default();
    assert_eq!(
        stats.record_kill(),
        KillProgress {
            total: 1,
            streak: 1
        }
    );
    assert_eq!(
        stats.record_kill(),
        KillProgress {
            total: 2,
            streak: 2
        }
    );
    assert_eq!(stats.best_kill_streak, 2);
    stats.reset_kill_streak();
    assert_eq!(stats.kill_streak, 0);
    assert_eq!(
        stats.record_kill(),
        KillProgress {
            total: 3,
            streak: 1
        }
    );
    assert_eq!(stats.best_kill_streak, 2);
}

#[test]
fn rankings_follow_all_tie_breaks() {
    let mut app = App::new();
    let board = BoardGrid::generate(2, 2, &GameConfig::default());
    app.insert_resource(TerritoryMap::from_board(&board))
        .insert_resource(board)
        .init_resource::<Rankings>()
        .init_resource::<SimulationEvents>()
        .add_systems(Update, update_rankings);
    for (id, alive, kills) in [(0, true, 1), (1, true, 3), (2, false, 9)] {
        app.world_mut().spawn((
            Competitor {
                id: CompetitorId(id),
                display_name: String::new(),
                kind: CompetitorKind::Npc,
                color_id: id,
                pattern_id: 0,
            },
            LifeState {
                status: if alive {
                    LifeStatus::Alive
                } else {
                    LifeStatus::Respawning
                },
                respawn_remaining: 0.0,
            },
            MatchStatistics { kills, ..default() },
        ));
    }
    app.update();
    let capacity_after_first_update = app.world().resource::<Rankings>().entries.capacity();
    let ranking_events_after_first_update = app
        .world()
        .resource::<SimulationEvents>()
        .0
        .iter()
        .filter(|event| matches!(event, SimulationEvent::RankingChanged))
        .count();
    app.update();
    assert_eq!(
        app.world().resource::<Rankings>().entries.capacity(),
        capacity_after_first_update
    );
    assert_eq!(
        app.world()
            .resource::<SimulationEvents>()
            .0
            .iter()
            .filter(|event| matches!(event, SimulationEvent::RankingChanged))
            .count(),
        ranking_events_after_first_update
    );
    let ids: Vec<_> = app
        .world()
        .resource::<Rankings>()
        .entries
        .iter()
        .map(|entry| entry.id.0)
        .collect();
    assert_eq!(ids, [1, 0, 2]);
}

#[test]
fn start_match_fills_empty_slots_with_npcs_and_disjoint_seeds() {
    let mut world = World::new();
    world.insert_resource(GameConfig::default());
    world.insert_resource(BoardGrid::default());
    world.insert_resource(SimulationEvents::default());
    let setup = MatchSetup {
        seed: 91,
        total_competitors: 8,
        humans: Vec::new(),
        replay_same_field: false,
    };
    start_match(&mut world, &setup);
    let mut query = world.query::<(&Competitor, &CompetitorMotion)>();
    let competitors: Vec<_> = query.iter(&world).collect();
    assert_eq!(competitors.len(), 8);
    assert!(
        competitors
            .iter()
            .all(|(competitor, _)| competitor.kind == CompetitorKind::Npc)
    );
    for (index, (_, first)) in competitors.iter().enumerate() {
        for (_, second) in &competitors[..index] {
            assert!(first.position.distance(second.position) > 5.5);
        }
    }
    let board = world.resource::<BoardGrid>();
    assert!(board.verify_counts());
    assert!(board.owner_counts[..8].iter().all(|count| *count > 0));
}

#[test]
fn attract_matches_start_six_npcs_without_a_countdown() {
    let mut world = World::new();
    world.insert_resource(GameConfig::default());
    world.insert_resource(BoardGrid::default());
    world.insert_resource(SimulationEvents::default());
    let setup = MatchSetup {
        seed: 0xA77A_C7A5_5EED,
        total_competitors: 6,
        humans: Vec::new(),
        replay_same_field: false,
    };
    start_match_for(&mut world, &setup, MatchPurpose::Attract);

    assert_eq!(
        world.resource::<MatchSession>().purpose,
        MatchPurpose::Attract
    );
    assert_eq!(world.resource::<MatchSession>().phase, MatchPhase::Running);
    let mut query = world.query::<&Competitor>();
    let competitors: Vec<_> = query.iter(&world).collect();
    assert_eq!(competitors.len(), 6);
    assert!(
        competitors
            .iter()
            .all(|competitor| competitor.kind == CompetitorKind::Npc)
    );
}

#[test]
fn attract_matches_do_not_trigger_victory_navigation() {
    let mut app = App::new();
    let board = BoardGrid::generate(1, 2, &GameConfig::default());
    let mut territory = TerritoryMap::from_board(&board);
    territory.territories[0] = territory.arena.clone();
    app.init_resource::<NextState<AppState>>()
        .insert_resource(GameConfig::default())
        .insert_resource(board)
        .insert_resource(territory)
        .insert_resource(MatchSession {
            purpose: MatchPurpose::Attract,
            phase: MatchPhase::Running,
            ..default()
        })
        .init_resource::<SimulationEvents>()
        .add_systems(Update, check_victory);

    app.update();
    assert_eq!(
        app.world().resource::<MatchSession>().phase,
        MatchPhase::Running
    );
    assert_eq!(app.world().resource::<MatchSession>().winner, None);
    assert!(app.world().resource::<SimulationEvents>().0.is_empty());
}

#[test]
fn death_immediately_clears_territory_and_active_trail() {
    let config = GameConfig::default();
    let mut board = BoardGrid::generate(17, 2, &config);
    let id = CompetitorId(0);
    board.claim_disk(Vec2::ZERO, config.starting_territory_radius, id);
    let anchor = board.world_to_cell(Vec2::ZERO).unwrap();
    let mut trail = ActiveTrail::new(id, anchor, Vec2::ZERO, Vec2::X);
    trail.append_exact(Vec2::X);
    update_trail_raster(&mut board, &mut trail, config.trail_width);

    let mut app = App::new();
    let mut territory = TerritoryMap::from_board(&board);
    territory.seed_owner(Vec2::ZERO, config.starting_territory_radius, id);
    app.insert_resource(config)
        .insert_resource(board)
        .insert_resource(territory)
        .insert_resource(PendingDeaths(vec![crate::combat::TrailCollisionIntent {
            victim: id,
            killer: None,
            impact_time: 0.5,
        }]))
        .init_resource::<SimulationEvents>()
        .add_systems(Update, resolve_deaths);
    let entity = app
        .world_mut()
        .spawn((
            Competitor {
                id,
                display_name: "Test".into(),
                kind: CompetitorKind::Npc,
                color_id: 0,
                pattern_id: 0,
            },
            LifeState::alive(),
            MatchStatistics::default(),
            trail,
        ))
        .id();
    app.update();

    let world = app.world();
    assert!(!world.entity(entity).get::<LifeState>().unwrap().is_alive());
    assert!(world.entity(entity).get::<ActiveTrail>().is_none());
    assert_eq!(world.resource::<BoardGrid>().owner_counts[id.index()], 0);
    assert!(world.resource::<BoardGrid>().verify_counts());
}

#[test]
fn credited_kill_updates_streak_and_emits_presentation_event() {
    let config = GameConfig::default();
    let board = BoardGrid::generate(23, 2, &config);
    let territory = TerritoryMap::from_board(&board);
    let mut app = App::new();
    app.insert_resource(config)
        .insert_resource(board)
        .insert_resource(territory)
        .insert_resource(PendingDeaths(vec![crate::combat::TrailCollisionIntent {
            victim: CompetitorId(0),
            killer: Some(CompetitorId(1)),
            impact_time: 0.25,
        }]))
        .init_resource::<SimulationEvents>()
        .add_systems(Update, resolve_deaths);
    for id in [0, 1] {
        app.world_mut().spawn((
            Competitor {
                id: CompetitorId(id),
                display_name: format!("Player {id}"),
                kind: CompetitorKind::Npc,
                color_id: id,
                pattern_id: id,
            },
            LifeState::alive(),
            MatchStatistics {
                kill_streak: (id == 0) as u32 * 2,
                ..default()
            },
        ));
    }
    app.update();

    let mut query = app
        .world_mut()
        .query::<(&Competitor, &MatchStatistics, &LifeState)>();
    let world = app.world();
    let killer = query
        .iter(world)
        .find(|(competitor, _, _)| competitor.id == CompetitorId(1))
        .unwrap();
    let victim = query
        .iter(world)
        .find(|(competitor, _, _)| competitor.id == CompetitorId(0))
        .unwrap();
    assert_eq!(killer.1.kills, 1);
    assert_eq!(killer.1.kill_streak, 1);
    assert_eq!(victim.1.kill_streak, 0);
    assert!(!victim.2.is_alive());
    assert!(
        app.world()
            .resource::<SimulationEvents>()
            .0
            .iter()
            .any(|event| matches!(
                event,
                SimulationEvent::Kill {
                    killer: CompetitorId(1),
                    progress: KillProgress {
                        total: 1,
                        streak: 1
                    }
                }
            ))
    );

    app.world_mut()
        .resource_mut::<PendingDeaths>()
        .0
        .push(crate::combat::TrailCollisionIntent {
            victim: CompetitorId(0),
            killer: Some(CompetitorId(1)),
            impact_time: 0.25,
        });
    app.update();
    let mut query = app.world_mut().query::<(&Competitor, &MatchStatistics)>();
    let killer = query
        .iter(app.world())
        .find(|(competitor, _)| competitor.id == CompetitorId(1))
        .unwrap();
    assert_eq!(
        killer.1.kills, 1,
        "an already-dead victim cannot grant another kill"
    );
}

#[test]
fn leaving_owned_seed_ends_protection_after_minimum_time() {
    let config = GameConfig::default();
    let mut board = BoardGrid::generate(7, 2, &config);
    let id = CompetitorId(0);
    board.claim_disk(Vec2::ZERO, config.starting_territory_radius, id);
    let mut protection = SpawnProtection {
        remaining: config.spawn_protection_seconds,
        elapsed: 0.49,
    };
    advance_spawn_protection(
        &mut protection,
        id,
        Vec2::new(4.0, 0.0),
        &{
            let mut territory = TerritoryMap::from_board(&board);
            territory.seed_owner(Vec2::ZERO, config.starting_territory_radius, id);
            territory
        },
        &config,
        0.02,
    );
    assert_eq!(protection.remaining, 0.0);
}

#[test]
fn respawn_seed_displacement_is_credited() {
    let config = GameConfig::default();
    let mut board = BoardGrid::generate(8, 2, &config);
    let victim = CompetitorId(0);
    let respawning = CompetitorId(1);
    board.claim_disk(Vec2::ZERO, 1.0, victim);
    let mut territory = TerritoryMap::from_board(&board);
    territory.clear_owner(victim);
    territory.seed_owner(Vec2::ZERO, 1.0, victim);
    let mut credits = DisplacementCredits::default();
    claim_respawn_seed(
        &mut board,
        &mut territory,
        Vec2::ZERO,
        config.starting_territory_radius,
        respawning,
        &[(victim, Vec2::ZERO)],
        &mut credits,
    );
    assert_eq!(board.owner_counts[victim.index()], 0);
    assert_eq!(credits.0, vec![(victim, respawning)]);
}

#[test]
fn severed_occupied_island_runs_the_full_displacement_lifecycle() {
    let config = GameConfig::default();
    let mut board = BoardGrid::generate(31, 2, &config);
    let mut territory = TerritoryMap::from_board(&board);
    let victim = CompetitorId(0);
    let attacker = CompetitorId(1);
    let rectangle = |min: Vec2, max: Vec2| {
        crate::geometry::MultiPolygon::from_outer(&[
            min,
            Vec2::new(max.x, min.y),
            max,
            Vec2::new(min.x, max.y),
        ])
    };

    territory.seed_owner(Vec2::new(-5.0, 0.0), 1.5, victim);
    let dumbbell = rectangle(Vec2::new(-7.0, -2.0), Vec2::new(-2.0, 2.0))
        .union(&rectangle(Vec2::new(-2.0, -0.5), Vec2::new(2.0, 0.5)))
        .union(&rectangle(Vec2::new(2.0, -2.0), Vec2::new(7.0, 2.0)));
    territory.apply_claim(victim, dumbbell);
    territory.seed_owner(Vec2::new(0.0, -4.0), 1.5, attacker);
    territory.rebuild_sample_cache(&mut board);

    let mut app = App::new();
    app.insert_resource(config.clone())
        .insert_resource(board)
        .insert_resource(territory)
        .init_resource::<PendingCaptures>()
        .init_resource::<DisplacementCredits>()
        .init_resource::<SimulationEvents>()
        .insert_resource(Time::<Fixed>::from_hz(config.fixed_hz))
        .add_systems(
            Update,
            (resolve_captures, resolve_territory_consequences).chain(),
        );

    let spawn_competitor = |world: &mut World, id, position| {
        let last_owned = world
            .resource::<BoardGrid>()
            .world_to_cell(position)
            .expect("fixture positions are on the board");
        world
            .spawn((
                Competitor {
                    id,
                    display_name: format!("Player {}", id.0),
                    kind: CompetitorKind::Npc,
                    color_id: id.0,
                    pattern_id: id.0,
                },
                CompetitorMotion::new(position, Vec2::Y),
                LifeState::alive(),
                SpawnProtection {
                    remaining: 0.0,
                    elapsed: 1.0,
                },
                TerritoryRecord::default(),
                MatchStatistics::default(),
                LastOwnedCell(last_owned),
            ))
            .id()
    };
    let victim_entity = spawn_competitor(app.world_mut(), victim, Vec2::new(5.0, 0.0));
    let attacker_entity = spawn_competitor(app.world_mut(), attacker, Vec2::new(0.0, 2.0));
    let start = Vec2::new(0.0, -3.0);
    let mut trail = ActiveTrail::new(
        attacker,
        app.world()
            .resource::<BoardGrid>()
            .world_to_cell(start)
            .expect("trail starts on the board"),
        start,
        Vec2::Y,
    );
    trail.append_exact(Vec2::new(0.0, 2.0));
    app.world_mut()
        .entity_mut(attacker_entity)
        .insert(trail.clone());
    app.world_mut()
        .resource_mut::<PendingCaptures>()
        .0
        .push(PendingCapture {
            player: attacker,
            entity: attacker_entity,
            time: 0.5,
            trail,
        });

    app.update();

    let world = app.world();
    assert!(
        !world
            .entity(victim_entity)
            .get::<LifeState>()
            .unwrap()
            .is_alive()
    );
    assert_eq!(
        world
            .entity(victim_entity)
            .get::<MatchStatistics>()
            .unwrap()
            .deaths,
        1
    );
    assert_eq!(
        world
            .entity(attacker_entity)
            .get::<MatchStatistics>()
            .unwrap()
            .kills,
        1
    );
    assert!(
        world
            .resource::<SimulationEvents>()
            .0
            .iter()
            .any(|event| matches!(
                event,
                SimulationEvent::Death {
                    victim: event_victim,
                    killer: Some(event_killer),
                    cause: DeathCause::Displaced,
                } if *event_victim == victim && *event_killer == attacker
            ))
    );
}

#[test]
fn respawn_resets_the_next_trail_anchor_to_the_new_seed() {
    let board = BoardGrid::generate(17, 2, &GameConfig::default());
    let position = board.cell_center(board.cell(board.spawn_candidates[0]));
    let mut motion = CompetitorMotion::new(Vec2::new(99.0, 99.0), Vec2::X);
    let mut last_owned = LastOwnedCell(crate::board::Cell::new(-1, -1));

    reset_respawn_anchor(&board, &mut motion, &mut last_owned, position);

    assert_eq!(motion.position, position);
    assert_eq!(motion.previous_position, position);
    assert_eq!(last_owned.0, board.world_to_cell(position).unwrap());
}

#[test]
fn cleanup_despawns_match_entities_but_preserves_setup() {
    let mut app = App::new();
    let setup = MatchSetup {
        seed: 11,
        total_competitors: 4,
        humans: Vec::new(),
        replay_same_field: true,
    };
    app.insert_resource(BoardGrid::generate(11, 4, &GameConfig::default()))
        .init_resource::<TerritoryMap>()
        .insert_resource(MatchSession::default())
        .insert_resource(Rankings::default())
        .insert_resource(SimulationEvents::default())
        .insert_resource(setup.clone())
        .add_systems(Update, cleanup_match);
    let entity = app
        .world_mut()
        .spawn(Competitor {
            id: CompetitorId(0),
            display_name: "Old".into(),
            kind: CompetitorKind::Npc,
            color_id: 0,
            pattern_id: 0,
        })
        .id();
    app.update();
    assert!(app.world().get_entity(entity).is_err());
    assert_eq!(app.world().resource::<MatchSetup>(), &setup);
    assert!(app.world().resource::<BoardGrid>().is_empty());
}

#[test]
fn npc_one_hour_headless_soak_preserves_authoritative_invariants() {
    let config = GameConfig {
        fixed_hz: 10.0,
        cell_size: 2.0,
        starting_territory_radius: 4.0,
        spawn_protection_seconds: 0.2,
        spawn_protection_minimum_seconds: 0.1,
        respawn_base_seconds: 0.5,
        ..default()
    };
    // The accelerated test clock is 10 Hz; production remains 60 Hz.
    let mut app = App::new();
    app.insert_resource(config.clone())
        .insert_resource(BoardGrid::default())
        .insert_resource(MatchSession::default())
        .insert_resource(Rankings::default())
        .insert_resource(SimulationEvents::default())
        .insert_resource(PendingDeaths::default())
        .insert_resource(PendingCaptures::default())
        .insert_resource(DisplacementCredits::default())
        .insert_resource(NextState::<AppState>::default())
        .insert_resource(Time::<Fixed>::from_hz(10.0))
        .add_systems(
            Update,
            (
                npc_think,
                move_competitors,
                extend_trails,
                detect_trail_collisions,
                resolve_deaths,
                detect_closures,
                resolve_captures,
                resolve_territory_consequences,
                advance_respawns,
                update_rankings,
            )
                .chain(),
        );
    start_match(
        app.world_mut(),
        &MatchSetup {
            seed: 5_711,
            total_competitors: 4,
            humans: Vec::new(),
            replay_same_field: false,
        },
    );
    app.world_mut().resource_mut::<MatchSession>().phase = MatchPhase::Running;

    // Seed one deterministic, real swept trail cut so the soak necessarily exercises death
    // and respawn even if the autonomous competitors happen not to meet early.
    let entities: Vec<(CompetitorId, Entity)> = {
        let world = app.world_mut();
        let mut query = world.query::<(Entity, &Competitor)>();
        query
            .iter(world)
            .map(|(entity, c)| (c.id, entity))
            .collect()
    };
    let victim = entities.iter().find(|(id, _)| id.0 == 0).unwrap().1;
    let attacker = entities.iter().find(|(id, _)| id.0 == 1).unwrap().1;
    {
        let mut entity = app.world_mut().entity_mut(attacker);
        entity.get_mut::<NpcController>().unwrap().think_remaining = 1.0;
        let mut motion = entity.get_mut::<CompetitorMotion>().unwrap();
        *motion = CompetitorMotion::new(Vec2::new(-0.4, 0.0), Vec2::X);
        let mut intent = entity.get_mut::<SteeringIntent>().unwrap();
        intent.desired_direction = Vec2::X;
        intent.magnitude = 1.0;
    }
    let mut forced_trail = ActiveTrail::new(
        CompetitorId(0),
        app.world().entity(victim).get::<LastOwnedCell>().unwrap().0,
        Vec2::new(0.0, -2.0),
        Vec2::Y,
    );
    forced_trail.append_exact(Vec2::new(0.0, 2.0));
    {
        let mut board = app.world_mut().resource_mut::<BoardGrid>();
        update_trail_raster(&mut board, &mut forced_trail, config.trail_width);
    }
    app.world_mut().entity_mut(victim).insert(forced_trail);
    for entity in entities.iter().map(|(_, entity)| *entity) {
        app.world_mut()
            .entity_mut(entity)
            .get_mut::<SpawnProtection>()
            .unwrap()
            .remaining = 0.0;
    }

    const TICKS: usize = 3_600 * 10;
    for tick in 0..TICKS {
        app.world_mut()
            .resource_mut::<Time<Fixed>>()
            .advance_by(Duration::from_millis(100));
        app.update();
        if tick.is_multiple_of(1_000) {
            assert!(app.world().resource::<BoardGrid>().verify_counts());
        }
    }

    let (owner_counts, active_trail_bits, board_len) = {
        let board = app.world().resource::<BoardGrid>();
        assert!(board.verify_counts());
        (
            board.owner_counts,
            board.active_trail_bits.clone(),
            board.len(),
        )
    };
    let mut expected_bits = vec![0_u16; board_len];
    let (captures, deaths, kills) = {
        let mut query = app.world_mut().query::<(
            &Competitor,
            &LifeState,
            &MatchStatistics,
            Option<&ActiveTrail>,
        )>();
        let world = app.world();
        let mut captures = 0;
        let mut deaths = 0;
        let mut kills = 0;
        for (competitor, life, statistics, trail) in query.iter(world) {
            captures += statistics.captures_completed;
            deaths += statistics.deaths;
            kills += statistics.kills;
            if !life.is_alive() {
                assert_eq!(owner_counts[competitor.id.index()], 0);
                assert!(trail.is_none());
            }
            if let Some(trail) = trail {
                for &cell in &trail.cells {
                    expected_bits[cell] |= 1 << competitor.id.index();
                }
            }
        }
        (captures, deaths, kills)
    };
    assert_eq!(active_trail_bits, expected_bits);
    assert!(
        captures > 0,
        "NPCs should complete captures during the soak"
    );
    assert!(deaths > 0, "the forced swept cut should record a death");
    assert!(
        kills > 0,
        "the swept trail cut should award NPC kill credit"
    );
    assert!(
        app.world()
            .resource::<SimulationEvents>()
            .0
            .iter()
            .any(|event| matches!(event, SimulationEvent::Respawn { .. }))
    );
}
