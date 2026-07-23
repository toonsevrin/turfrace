use super::*;
use crate::{lobby::MatchSetup, match_game::lifecycle::start_match};
use std::time::Duration;

#[test]
fn exact_victory_does_not_round() {
    let mut app = App::new();
    let board = BoardGrid::generate(1, 2, &GameConfig::default());
    let mut territory = TerritoryMap::from_board(&board);
    app.init_resource::<NextState<AppState>>()
        .insert_resource(board)
        .insert_resource(territory.clone())
        .init_resource::<MatchSession>()
        .init_resource::<SimulationEvents>()
        .add_systems(Update, check_victory);
    {
        let sliver = crate::geometry::MultiPolygon::from_outer(&[
            Vec2::new(-0.02, -100.0),
            Vec2::new(0.02, -100.0),
            Vec2::new(0.02, 100.0),
            Vec2::new(-0.02, 100.0),
        ]);
        territory.territories[0] = territory.arena.difference(&sliver);
        *app.world_mut().resource_mut::<TerritoryMap>() = territory.clone();
    }
    app.update();
    assert_eq!(app.world().resource::<MatchSession>().winner, None);
    {
        territory.territories[0] = territory.arena.clone();
        *app.world_mut().resource_mut::<TerritoryMap>() = territory;
    }
    app.update();
    assert_eq!(
        app.world().resource::<MatchSession>().winner,
        Some(CompetitorId(0))
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
    territory.claim_disk(Vec2::ZERO, config.starting_territory_radius, id);
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
            territory.claim_disk(Vec2::ZERO, config.starting_territory_radius, id);
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
    territory.claim_disk(Vec2::ZERO, 1.0, victim);
    let mut credits = DisplacementCredits::default();
    claim_respawn_seed(
        &mut board,
        &mut territory,
        Vec2::ZERO,
        config.starting_territory_radius,
        respawning,
        &mut credits,
    );
    assert_eq!(board.owner_counts[victim.index()], 0);
    assert_eq!(credits.0, vec![(victim, respawning)]);
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
