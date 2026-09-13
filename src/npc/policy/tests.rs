use super::*;
use crate::{board::BoardGrid, config::GameConfig, territory_map::TerritoryMap};

fn context<'a>(
    board: &'a BoardGrid,
    territory: &'a TerritoryMap,
    config: &'a GameConfig,
) -> NpcTickContext<'a> {
    NpcTickContext {
        board,
        territory,
        config,
        rank: 1,
        tick: 0,
        speed: config.player_speed,
        last_owned: crate::board::Cell::new(0, 0),
        own_trail: None,
    }
}

fn empty_observation() -> NpcObservation {
    NpcObservation {
        position: Vec2::ZERO,
        heading: Vec2::X,
        speed: 0.0,
        protected: false,
        owns_current_cell: true,
        trail_length: 0.0,
        edge_distance: 20.0,
        inward_direction: Vec2::Y,
        home: None,
        rivals: [None; NPC_VISIBLE_RIVAL_CAP],
        segments: [None; NPC_VISIBLE_SEGMENT_CAP],
        encounter: NpcEncounter::default(),
    }
}

#[test]
fn finite_segment_intercept_uses_swept_contact_and_stays_on_segment() {
    let config = GameConfig::default();
    let board = BoardGrid::generate(91, 2, &config);
    let territory = TerritoryMap::from_board(&board);
    let segment = NpcVisibleSegment {
        owner: CompetitorId(1),
        segment: 4,
        start: Vec2::new(4.0, -4.0),
        end: Vec2::new(4.0, 4.0),
        nearest_point: Vec2::new(4.0, 0.0),
        relative: Vec2::new(4.0, 0.0),
        distance: 4.0,
        tangent: Vec2::Y,
        own: false,
    };
    let observation = NpcObservation {
        position: Vec2::ZERO,
        heading: Vec2::X,
        speed: config.player_speed,
        protected: false,
        owns_current_cell: true,
        trail_length: 0.0,
        edge_distance: 20.0,
        inward_direction: Vec2::Y,
        home: None,
        rivals: [None; NPC_VISIBLE_RIVAL_CAP],
        segments: [None; NPC_VISIBLE_SEGMENT_CAP],
        encounter: NpcEncounter::default(),
    };
    let result = best_segment_intercept(
        &segment,
        &observation,
        &context(&board, &territory, &config),
        f32::INFINITY,
    )
    .expect("a reachable finite segment should be intercepted");
    assert!(result.0.x >= segment.start.x && result.0.x <= segment.end.x);
    assert!(result.1.is_finite() && result.1 > 0.0);
    let mut observation = observation;
    observation.segments[0] = Some(segment);
    let profile = NpcProfile {
        policy: NpcPolicy::Builder(BuilderPolicy {
            shape: BuilderShape::Roamer,
            side: TurnSide::Left,
        }),
        competence: NpcCompetence { skill: 0.8 },
    };
    let context = context(&board, &territory, &config);
    let tactic = choose_hunter_tactic(
        CompetitorId(0),
        &observation,
        &context,
        profile,
        &NpcEventMemory::default(),
    )
    .expect("winning visible segment should be selected");
    assert!(winning_segment_intercept(
        &tactic,
        &observation,
        &context,
        profile.competence
    ));
    assert_eq!(tactic.phase, TacticPhase::Committing);
}

#[test]
fn precomputed_eta_preserves_segment_intercept_result() {
    let config = GameConfig::default();
    let board = BoardGrid::generate(91, 2, &config);
    let territory = TerritoryMap::from_board(&board);
    let context = context(&board, &territory, &config);
    let segment = NpcVisibleSegment {
        owner: CompetitorId(1),
        segment: 4,
        start: Vec2::new(4.0, -4.0),
        end: Vec2::new(4.0, 4.0),
        nearest_point: Vec2::new(4.0, 0.0),
        distance: 4.0,
        ..default()
    };
    let observation = empty_observation();
    let uncached_eta = rival_home_reentry_eta_for_owner(
        segment.owner,
        &observation,
        &context,
        NpcCompetence { skill: 0.8 },
    );
    let mut cache = Vec::new();
    let cached_eta = cached_rival_home_reentry_eta(
        segment.owner,
        &observation,
        &context,
        NpcCompetence { skill: 0.8 },
        &mut cache,
    );

    assert_eq!(
        best_segment_intercept(&segment, &observation, &context, uncached_eta),
        best_segment_intercept(&segment, &observation, &context, cached_eta),
    );
}

#[test]
fn hunter_eta_cache_reuses_the_same_owner_result() {
    let config = GameConfig::default();
    let board = BoardGrid::generate(91, 2, &config);
    let mut territory = TerritoryMap::from_board(&board);
    territory.seed_owner(Vec2::new(10.0, 0.0), 3.0, CompetitorId(1));
    let context = context(&board, &territory, &config);
    let rival = NpcVisibleRival {
        id: CompetitorId(1),
        relative: Vec2::new(4.0, 0.0),
        distance: 4.0,
        heading: Vec2::X,
        speed: config.player_speed,
        ..default()
    };
    let mut observation = empty_observation();
    observation.rivals[0] = Some(rival);
    let competence = NpcCompetence { skill: 0.8 };
    let mut cache = Vec::new();

    let first =
        cached_rival_home_reentry_eta(rival.id, &observation, &context, competence, &mut cache);
    let second =
        cached_rival_home_reentry_eta(rival.id, &observation, &context, competence, &mut cache);

    assert_eq!(
        first,
        rival_home_reentry_eta_for_owner(rival.id, &observation, &context, competence)
    );
    assert_eq!(second, first);
    assert_eq!(cache, vec![(rival.id, first)]);
}

#[test]
fn winning_intercept_requires_the_selected_segment_index() {
    let config = GameConfig::default();
    let board = BoardGrid::generate(91, 2, &config);
    let territory = TerritoryMap::from_board(&board);
    let valid = NpcVisibleSegment {
        owner: CompetitorId(1),
        segment: 4,
        start: Vec2::new(4.0, -4.0),
        end: Vec2::new(4.0, 4.0),
        nearest_point: Vec2::new(4.0, 0.0),
        distance: 4.0,
        ..default()
    };
    let invalid_selected = NpcVisibleSegment {
        segment: 99,
        start: Vec2::ZERO,
        end: Vec2::ZERO,
        ..valid
    };
    let mut observation = NpcObservation {
        position: Vec2::ZERO,
        heading: Vec2::X,
        speed: config.player_speed,
        protected: false,
        owns_current_cell: true,
        trail_length: 0.0,
        edge_distance: 20.0,
        inward_direction: Vec2::Y,
        home: None,
        rivals: [None; NPC_VISIBLE_RIVAL_CAP],
        segments: [None; NPC_VISIBLE_SEGMENT_CAP],
        encounter: NpcEncounter::default(),
    };
    observation.segments[0] = Some(valid);
    observation.segments[1] = Some(invalid_selected);
    let tactic = NpcTactic::new(
        NpcTacticKind::Hunt(HuntTarget::Segment {
            owner: CompetitorId(1),
            segment: invalid_selected.segment,
        }),
        NpcRoute::from_points(
            &[Vec2::X],
            RouteTarget::Segment {
                owner: CompetitorId(1),
                index: invalid_selected.segment,
            },
        ),
        0,
    );
    assert!(!winning_segment_intercept(
        &tactic,
        &observation,
        &context(&board, &territory, &config),
        NpcCompetence { skill: 0.8 },
    ));
}

#[test]
fn hunter_target_policies_do_not_alias_trail_filtering() {
    let segment = NpcVisibleSegment {
        owner: CompetitorId(1),
        distance: 100.0,
        ..default()
    };
    let observation = empty_observation();
    let competence = NpcCompetence { skill: 0.8 };
    assert!(hunter_segment_is_candidate(
        segment,
        HunterTarget::Trail,
        &observation,
        competence,
    ));
    assert!(!hunter_segment_is_candidate(
        segment,
        HunterTarget::Opportunistic,
        &observation,
        competence,
    ));
}

#[test]
fn roamer_builder_emits_genuine_open_space_route() {
    let config = GameConfig::default();
    let board = BoardGrid::generate(91, 2, &config);
    let mut territory = TerritoryMap::from_board(&board);
    territory.seed_owner(Vec2::ZERO, 10.0, CompetitorId(0));
    let observation = NpcObservation {
        position: Vec2::ZERO,
        heading: Vec2::X,
        speed: config.player_speed,
        protected: false,
        owns_current_cell: true,
        trail_length: 0.0,
        edge_distance: 20.0,
        inward_direction: Vec2::Y,
        home: None,
        rivals: [None; NPC_VISIBLE_RIVAL_CAP],
        segments: [None; NPC_VISIBLE_SEGMENT_CAP],
        encounter: NpcEncounter::default(),
    };
    let profile = NpcProfile {
        policy: NpcPolicy::Builder(BuilderPolicy {
            shape: BuilderShape::Roamer,
            side: TurnSide::Left,
        }),
        competence: NpcCompetence { skill: 0.8 },
    };
    let mut scratch = CaptureScratch::default();
    let tactic = choose_builder_tactic(
        CompetitorId(0),
        &observation,
        &NpcTickContext {
            board: &board,
            territory: &territory,
            config: &config,
            rank: 1,
            tick: 0,
            speed: config.player_speed,
            last_owned: crate::board::Cell::new(0, 0),
            own_trail: None,
        },
        profile,
        &NpcEventMemory::new(),
        &mut scratch,
    )
    .expect("seeded territory should provide an open-space route");
    assert_eq!(tactic.kind, NpcTacticKind::Roam);
    assert_eq!(tactic.route.target, RouteTarget::OpenSpace);
    assert_eq!(tactic.route.count, 1);
    assert!(
        tactic
            .route
            .final_point()
            .is_some_and(|point| territory.owner_at(point) == OwnerId::UNCLAIMED)
    );
}

#[test]
fn late_abort_delays_but_does_not_remove_raid_trail_abort() {
    let profile = NpcProfile {
        policy: NpcPolicy::Raider(RaiderPolicy {
            objective: RaidObjective::Leader,
            shape: RaidShape::Hook,
        }),
        competence: NpcCompetence { skill: 0.8 },
    };
    let mut observation = empty_observation();
    let mut tactic = NpcTactic::new(
        NpcTacticKind::Raid(RaidTarget::Leader(CompetitorId(1))),
        NpcRoute::from_points(&[Vec2::X], RouteTarget::OpenSpace),
        0,
    );
    tactic.next_interrupt_tick = 0;

    observation.trail_length = raid_abort_trail_limit(profile, None) + 0.1;
    assert!(tactic_interrupted(&tactic, &observation, profile, 0));

    tactic.mistake = Some(NpcMistake::LateAbort);
    assert!(!tactic_interrupted(&tactic, &observation, profile, 0));
    observation.trail_length = raid_abort_trail_limit(profile, tactic.mistake) + 0.1;
    assert!(tactic_interrupted(&tactic, &observation, profile, 0));
}

#[test]
fn raid_budget_continues_only_for_safe_committed_reentry() {
    let config = GameConfig::default();
    let board = BoardGrid::generate(91, 2, &config);
    let mut territory = TerritoryMap::from_board(&board);
    territory.seed_owner(Vec2::ZERO, 3.0, CompetitorId(0));
    territory.seed_owner(Vec2::new(12.0, 0.0), 3.0, CompetitorId(1));
    let context = context(&board, &territory, &config);
    let profile = NpcProfile {
        policy: NpcPolicy::Raider(RaiderPolicy {
            objective: RaidObjective::Leader,
            shape: RaidShape::Hook,
        }),
        competence: NpcCompetence { skill: 0.8 },
    };
    let mut observation = empty_observation();
    observation.position = Vec2::new(5.0, 0.0);
    observation.owns_current_cell = false;
    observation.trail_length = raid_abort_trail_limit(profile, None) + 1.0;
    let safe_route = NpcRoute::from_points(
        &[Vec2::new(8.0, 0.0), Vec2::new(12.0, 0.0), Vec2::ZERO],
        RouteTarget::EnemyBorder(CompetitorId(1)),
    );
    let mut safe = NpcTactic::new(
        NpcTacticKind::Raid(RaidTarget::Border {
            owner: CompetitorId(1),
            point: Vec2::new(12.0, 0.0),
        }),
        safe_route,
        0,
    );
    safe.phase = TacticPhase::Committing;
    safe.next_interrupt_tick = 0;
    let mut scratch = CaptureScratch::default();
    assert!(!tactic_interrupted_with_context(
        &safe,
        &observation,
        profile,
        0,
        CompetitorId(0),
        CompetitorMotion::new(observation.position, observation.heading),
        &context,
        &mut scratch,
    ));

    let unsafe_route = NpcRoute::from_points(
        &[
            Vec2::new(8.0, 0.0),
            Vec2::new(12.0, 0.0),
            Vec2::new(6.0, 8.0),
        ],
        RouteTarget::EnemyBorder(CompetitorId(1)),
    );
    let unsafe_tactic = NpcTactic {
        route: unsafe_route,
        ..safe
    };
    assert!(tactic_interrupted_with_context(
        &unsafe_tactic,
        &observation,
        profile,
        0,
        CompetitorId(0),
        CompetitorMotion::new(observation.position, observation.heading),
        &context,
        &mut scratch,
    ));
}

#[test]
fn rival_return_eta_starts_at_observed_body_not_cut_target() {
    let config = GameConfig::default();
    let board = BoardGrid::generate(91, 2, &config);
    let mut territory = TerritoryMap::from_board(&board);
    territory.seed_owner(Vec2::new(10.0, 0.0), 3.0, CompetitorId(1));
    let context = context(&board, &territory, &config);
    let rival = NpcVisibleRival {
        id: CompetitorId(1),
        relative: Vec2::new(4.0, 0.0),
        distance: 4.0,
        heading: Vec2::X,
        speed: config.player_speed,
        ..default()
    };
    let eta = rival_home_reentry_eta(
        Vec2::ZERO,
        rival,
        rival.id,
        &context,
        NpcCompetence { skill: 0.8 },
    );
    assert!(eta.is_finite() && eta > 0.0);
}
