use super::*;
use crate::{
    board::{BoardGrid, Cell},
    config::GameConfig,
    movement::CompetitorMotion,
    territory_map::TerritoryMap,
    trail::ActiveTrail,
};
use bevy::prelude::*;
use std::collections::HashMap;

const RETURN_PREDICTION_CACHE_CAP: usize = FORECAST_ROUTE_STEP_CAP;

#[derive(Clone, Copy)]
struct ReturnThreat {
    motion: CompetitorMotion,
    speed: f32,
}

struct ReturnForecastCache {
    threat: Option<ReturnThreat>,
    predictions: HashMap<u32, Vec2>,
}

impl ReturnForecastCache {
    fn new(threat: Option<ReturnThreat>) -> Self {
        Self {
            threat,
            predictions: HashMap::new(),
        }
    }

    fn threat_position(&mut self, context: &ReturnPlanContext<'_>, elapsed: f32) -> Option<Vec2> {
        let threat = self.threat?;
        let key = elapsed.to_bits();
        if let Some(&position) = self.predictions.get(&key) {
            return Some(position);
        }
        let position = crate::movement::predict_position(
            threat.motion,
            Some(threat.motion.heading),
            context.territory,
            context.config,
            threat.speed,
            elapsed,
        );
        if self.predictions.len() < RETURN_PREDICTION_CACHE_CAP {
            self.predictions.insert(key, position);
        }
        Some(position)
    }
}

/// Resolve the observed body once for this planning decision. A marker without
/// a matching finite rival remains non-evidence, as in the original forecast.
fn resolve_return_threat(context: &ReturnPlanContext<'_>) -> Option<ReturnThreat> {
    let encounter = context.threat?;
    let owner = encounter.target.map(|target| match target {
        HuntTarget::Segment { owner, .. } | HuntTarget::Rival(owner) => owner,
    })?;
    context
        .rivals
        .iter()
        .flatten()
        .find(|rival| {
            rival.id == owner
                && rival.relative.is_finite()
                && rival.heading.is_finite()
                && rival.speed.is_finite()
                && (context.motion.position + rival.relative).is_finite()
        })
        .map(|rival| ReturnThreat {
            motion: CompetitorMotion::new(context.motion.position + rival.relative, rival.heading),
            speed: rival.speed,
        })
}

/// Borrowed inputs for selecting a safe owned re-entry. Threat observations are
/// deliberately supplied separately from the encounter marker: a marker alone
/// is not body evidence and cannot create a phantom rival.
pub struct ReturnPlanContext<'a> {
    pub board: &'a BoardGrid,
    pub territory: &'a TerritoryMap,
    pub config: &'a GameConfig,
    pub id: CompetitorId,
    pub motion: CompetitorMotion,
    pub speed: f32,
    pub last_owned: Cell,
    pub threat: Option<&'a NpcEncounter>,
    pub rivals: &'a [Option<NpcVisibleRival>],
    pub own_trail: Option<&'a ActiveTrail>,
    pub competence: NpcCompetence,
}

pub fn plan_safe_return(context: &ReturnPlanContext<'_>) -> NpcRoute {
    let radius = context.competence.sensor_horizon() * 1.6;
    let mut frontiers = Vec::with_capacity(FRONTIER_SAMPLE_CAP);
    context.territory.collect_owner_frontiers(
        context.motion.position,
        context.id,
        radius,
        &mut frontiers,
    );
    frontiers.truncate(FRONTIER_SAMPLE_CAP);
    let mut best: Option<(f32, f32, NpcRoute)> = None;
    let mut scratch = CaptureScratch::default();
    let mut forecast_cache = ReturnForecastCache::new(resolve_return_threat(context));
    let forecast_context = ForecastContext {
        board: context.board,
        territory: context.territory,
        config: context.config,
        id: context.id,
        motion: context.motion,
        speed: context.speed,
        own_trail: context.own_trail,
        competence: context.competence,
    };
    for frontier in frontiers {
        let inward = (-frontier.outward).normalize_or(Vec2::Y);
        let staging = frontier.position + inward * context.board.cell_size.max(1.0);
        let route = NpcRoute::from_points(&[staging, frontier.position], RouteTarget::OwnedGround);
        if !valid_return_geometry(&forecast_context, &route) {
            continue;
        }
        let result = forecast_return(
            context,
            &route,
            &mut scratch,
            best.as_ref().map(|(clearance, _, _)| *clearance),
            |position, elapsed| {
                forecast_cache
                    .threat_position(context, elapsed)
                    .map_or(f32::INFINITY, |threat_position| {
                        position.distance(threat_position)
                    })
            },
        );
        if !result.is_reached() {
            continue;
        }
        let (clearance, eta) = (result.clearance, result.elapsed);
        // Positive infinity is unbeatable under this planner's existing ranking:
        // even an infinite-clearance tie cannot replace it (inf - inf is NaN).
        // Without an observed threat this is the first safe route; forecasting
        // every remaining frontier cannot change the answer.
        if clearance == f32::INFINITY {
            return route;
        }
        let better = best.as_ref().is_none_or(|(old_clearance, old_eta, _)| {
            clearance > *old_clearance + 0.01
                || (clearance - *old_clearance).abs() <= 0.01 && eta < *old_eta
        });
        if better {
            best = Some((clearance, eta, route));
        }
    }
    if let Some((_, _, route)) = best {
        return route;
    }

    // LastOwnedCell is only an anchor after confirming its exact world point is
    // owned. Otherwise this is an explicit emergency route, never fake success.
    let anchor = context.board.cell_center(context.last_owned);
    if context.territory.owns(anchor, context.id) {
        let inward = context
            .territory
            .arena_inward_normal(context.motion.position);
        let staging = context
            .territory
            .arena_boundary()
            .project_inside(
                context.motion.position + inward * context.board.cell_size,
                context.config.collision_radius,
            )
            .unwrap_or(context.motion.position);
        let route = NpcRoute::from_points(&[staging, anchor], RouteTarget::OwnedGround);
        if valid_return_geometry(&forecast_context, &route)
            && return_forecast_clearance(context, &route, &mut scratch, &mut forecast_cache)
                .is_reached()
        {
            return route;
        }
    }
    let inward = context
        .territory
        .arena_inward_normal(context.motion.position)
        .normalize_or(Vec2::Y);
    let emergency = context
        .territory
        .arena_boundary()
        .project_inside(
            context.motion.position + inward * context.board.cell_size.max(1.0),
            context.config.collision_radius,
        )
        .unwrap_or(context.motion.position);
    NpcRoute::from_points(&[emergency], RouteTarget::EmergencyReturn)
}

fn valid_return_geometry(context: &ForecastContext<'_>, route: &NpcRoute) -> bool {
    if !geometry::valid_route_geometry(
        context.territory,
        context.config.collision_radius,
        route,
        context.motion.position,
    ) || !route
        .final_point()
        .is_some_and(|point| context.territory.owns(point, context.id))
        || !geometry::return_ownership_valid(
            context.territory,
            context.id,
            route,
            context.motion.position,
        )
    {
        return false;
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReturnForecastMode {
    Exhaustive,
    BranchAndBound,
}

fn return_forecast_request<'a, 'b>(
    context: &'a ReturnPlanContext<'b>,
    route: &'a NpcRoute,
) -> ForecastRequest<'a>
where
    'b: 'a,
{
    ForecastRequest {
        context: ForecastContext {
            board: context.board,
            territory: context.territory,
            config: context.config,
            id: context.id,
            motion: context.motion,
            speed: context.speed,
            own_trail: context.own_trail,
            competence: context.competence,
        },
        route,
        route_index: 0,
        max_ticks: FORECAST_ROUTE_STEP_CAP,
        goal: ForecastGoal::ReachEndpoint {
            waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
        },
    }
}

/// Forecast a return while optionally applying the exact ranking bound. A
/// missing or non-finite bound deliberately takes the old collect-then-score
/// path; in particular, it retains the no-threat and first-candidate behavior.
fn forecast_return<F>(
    context: &ReturnPlanContext<'_>,
    route: &NpcRoute,
    scratch: &mut CaptureScratch,
    best_clearance: Option<f32>,
    clearance_at: F,
) -> super::forecast::RouteForecast
where
    F: FnMut(Vec2, f32) -> f32,
{
    forecast_return_with_mode(
        context,
        route,
        scratch,
        best_clearance,
        ReturnForecastMode::BranchAndBound,
        clearance_at,
    )
}

fn forecast_return_with_mode<F>(
    context: &ReturnPlanContext<'_>,
    route: &NpcRoute,
    scratch: &mut CaptureScratch,
    best_clearance: Option<f32>,
    mode: ReturnForecastMode,
    mut clearance_at: F,
) -> super::forecast::RouteForecast
where
    F: FnMut(Vec2, f32) -> f32,
{
    let Some(best_clearance) = best_clearance
        .filter(|_| mode == ReturnForecastMode::BranchAndBound)
        .filter(|clearance| clearance.is_finite())
    else {
        return forecast_route(
            return_forecast_request(context, route),
            scratch,
            clearance_at,
        );
    };

    let mut clearance = f32::INFINITY;
    // The bounded scorer does not need materialized samples. Clear the
    // reusable collector so an earlier exhaustive candidate cannot linger.
    scratch.forecast_samples.clear();
    let mut result = super::forecast::forecast_path(
        return_forecast_request(context, route),
        scratch,
        |position, elapsed| {
            let sample_clearance = clearance_at(position, elapsed);
            clearance = clearance.min(sample_clearance);
            // A future running minimum cannot increase. Do not prune ties or
            // ambiguous floating-point values: both ETA and exact ranking
            // semantics still matter there.
            !(clearance < best_clearance && (clearance - best_clearance).abs() > 0.01)
        },
    );
    if result.is_viable() {
        result.clearance = clearance;
    }
    result
}

fn return_forecast_clearance(
    context: &ReturnPlanContext<'_>,
    route: &NpcRoute,
    scratch: &mut CaptureScratch,
    forecast_cache: &mut ReturnForecastCache,
) -> super::forecast::RouteForecast {
    forecast_return_with_mode(
        context,
        route,
        scratch,
        None,
        ReturnForecastMode::Exhaustive,
        |position, elapsed| {
            forecast_cache
                .threat_position(context, elapsed)
                .map_or(f32::INFINITY, |threat_position| {
                    position.distance(threat_position)
                })
        },
    )
}

/// Validate the still-unfinished part of a committed raid. The budget is not
/// relaxed: continuation is allowed only when the exact same fixed-tick,
/// held-input forecast reaches an owned re-entry after visiting the target.
pub(crate) fn committed_route_safe(
    context: &ForecastContext<'_>,
    route: &NpcRoute,
    route_index: usize,
    waypoint_threshold: f32,
    scratch: &mut CaptureScratch,
) -> bool {
    let Some(final_point) = route.final_point() else {
        return false;
    };
    let start = route_index.min(route.count as usize);
    if start >= route.count as usize {
        return false;
    }
    let continuation =
        NpcRoute::from_points(&route.points[start..route.count as usize], route.target);
    if !final_point.is_finite()
        || !context.territory.owns(final_point, context.id)
        || !geometry::valid_route_geometry(
            context.territory,
            context.config.collision_radius,
            &continuation,
            context.motion.position,
        )
        || !geometry::raid_ownership_valid(
            context.territory,
            context.id,
            &continuation,
            context.motion.position,
        )
        || !geometry::route_crosses_owner(
            context.territory,
            continuation.target,
            &continuation,
            context.board.cell_size,
        )
    {
        return false;
    }
    forecast_route(
        ForecastRequest {
            context: *context,
            route: &continuation,
            route_index: 0,
            max_ticks: FORECAST_ROUTE_STEP_CAP,
            goal: ForecastGoal::ReachEndpoint { waypoint_threshold },
        },
        scratch,
        |_, _| f32::INFINITY,
    )
    .is_reached()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (BoardGrid, TerritoryMap) {
        let board = BoardGrid::generate(91, 2, &GameConfig::default());
        let mut territory = TerritoryMap::from_board(&board);
        territory.seed_owner(Vec2::ZERO, 10.0, CompetitorId(0));
        (board, territory)
    }

    #[test]
    fn return_fallback_is_explicitly_emergency_when_no_owned_anchor_exists() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let route = plan_safe_return(&ReturnPlanContext {
            board: &board,
            territory: &territory,
            config: &config,
            id: CompetitorId(0),
            motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
            speed: config.player_speed,
            last_owned: Cell::new(0, 0),
            threat: None,
            rivals: &[],
            own_trail: None,
            competence: NpcCompetence { skill: 0.8 },
        });
        assert!(
            route.points[..route.count as usize]
                .iter()
                .all(|point| territory.arena_signed_distance(*point) >= config.collision_radius)
        );
        assert!(matches!(
            route.target,
            RouteTarget::EmergencyReturn | RouteTarget::OwnedGround
        ));
    }

    #[test]
    fn return_forecast_ignores_absent_and_segment_only_threats() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let route = NpcRoute::from_points(&[Vec2::new(4.0, 0.0)], RouteTarget::OwnedGround);
        let default_encounter = NpcEncounter {
            position: Vec2::ZERO,
            ..default()
        };
        let context = |threat, rivals| ReturnPlanContext {
            board: &board,
            territory: &territory,
            config: &config,
            id: CompetitorId(0),
            motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
            speed: config.player_speed,
            last_owned: Cell::new(0, 0),
            threat,
            rivals,
            own_trail: None,
            competence: NpcCompetence { skill: 0.8 },
        };
        let absent_context = context(Some(&default_encounter), &[]);
        let mut absent_cache = ReturnForecastCache::new(resolve_return_threat(&absent_context));
        let absent = return_forecast_clearance(
            &absent_context,
            &route,
            &mut CaptureScratch::default(),
            &mut absent_cache,
        );
        let segment_encounter = NpcEncounter {
            target: Some(HuntTarget::Segment {
                owner: CompetitorId(1),
                segment: 7,
            }),
            ..default_encounter
        };
        let segment_context = context(Some(&segment_encounter), &[]);
        let mut segment_cache = ReturnForecastCache::new(resolve_return_threat(&segment_context));
        let segment_only = return_forecast_clearance(
            &segment_context,
            &route,
            &mut CaptureScratch::default(),
            &mut segment_cache,
        );
        assert!(absent.is_reached() && absent.clearance.is_infinite());
        assert!(segment_only.is_reached() && segment_only.clearance.is_infinite());
    }

    #[test]
    fn cached_return_forecast_matches_uncached_prediction() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let route = NpcRoute::from_points(&[Vec2::new(4.0, 0.0)], RouteTarget::OwnedGround);
        let encounter = NpcEncounter {
            target: Some(HuntTarget::Rival(CompetitorId(1))),
            ..default()
        };
        let rivals = [Some(NpcVisibleRival {
            id: CompetitorId(1),
            relative: Vec2::X * 20.0,
            heading: -Vec2::X,
            speed: config.player_speed,
            ..default()
        })];
        let context = ReturnPlanContext {
            board: &board,
            territory: &territory,
            config: &config,
            id: CompetitorId(0),
            motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
            speed: config.player_speed,
            last_owned: Cell::new(0, 0),
            threat: Some(&encounter),
            rivals: &rivals,
            own_trail: None,
            competence: NpcCompetence { skill: 0.8 },
        };
        let mut cached_scratch = CaptureScratch::default();
        let mut cache = ReturnForecastCache::new(resolve_return_threat(&context));
        let cached = return_forecast_clearance(&context, &route, &mut cached_scratch, &mut cache);

        let threat = resolve_return_threat(&context).expect("test rival is observed");
        let mut uncached_scratch = CaptureScratch::default();
        let uncached = forecast_route(
            ForecastRequest {
                context: ForecastContext {
                    board: context.board,
                    territory: context.territory,
                    config: context.config,
                    id: context.id,
                    motion: context.motion,
                    speed: context.speed,
                    own_trail: context.own_trail,
                    competence: context.competence,
                },
                route: &route,
                route_index: 0,
                max_ticks: FORECAST_ROUTE_STEP_CAP,
                goal: ForecastGoal::ReachEndpoint {
                    waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
                },
            },
            &mut uncached_scratch,
            |position, elapsed| {
                position.distance(crate::movement::predict_position(
                    threat.motion,
                    Some(threat.motion.heading),
                    context.territory,
                    context.config,
                    threat.speed,
                    elapsed,
                ))
            },
        );

        assert_eq!(cached, uncached);
    }

    #[test]
    fn return_forecast_cache_reuses_exact_elapsed_bits() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let rival = NpcVisibleRival {
            id: CompetitorId(1),
            relative: Vec2::X * 20.0,
            heading: -Vec2::X,
            speed: config.player_speed,
            ..default()
        };
        let encounter = NpcEncounter {
            target: Some(HuntTarget::Rival(CompetitorId(1))),
            ..default()
        };
        let rivals = [Some(rival)];
        let context = ReturnPlanContext {
            board: &board,
            territory: &territory,
            config: &config,
            id: CompetitorId(0),
            motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
            speed: config.player_speed,
            last_owned: Cell::new(0, 0),
            threat: Some(&encounter),
            rivals: &rivals,
            own_trail: None,
            competence: NpcCompetence { skill: 0.8 },
        };
        let mut cache = ReturnForecastCache::new(resolve_return_threat(&context));
        let elapsed = 1.25_f32;
        let first = cache.threat_position(&context, elapsed);
        let second = cache.threat_position(&context, f32::from_bits(elapsed.to_bits()));

        assert_eq!(first, second);
        assert_eq!(cache.predictions.len(), 1);
        assert!(cache.predictions.contains_key(&elapsed.to_bits()));
    }

    #[test]
    fn return_forecast_uses_observed_rival_origin_and_speed() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let route = NpcRoute::from_points(&[Vec2::new(4.0, 0.0)], RouteTarget::OwnedGround);
        let encounter = NpcEncounter {
            target: Some(HuntTarget::Rival(CompetitorId(1))),
            ..default()
        };
        let stationary = NpcVisibleRival {
            id: CompetitorId(1),
            relative: Vec2::X * 20.0,
            heading: -Vec2::X,
            speed: 0.0,
            ..default()
        };
        let approaching = NpcVisibleRival {
            speed: config.player_speed,
            ..stationary
        };
        let context = |rivals| ReturnPlanContext {
            board: &board,
            territory: &territory,
            config: &config,
            id: CompetitorId(0),
            motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
            speed: config.player_speed,
            last_owned: Cell::new(0, 0),
            threat: Some(&encounter),
            rivals,
            own_trail: None,
            competence: NpcCompetence { skill: 0.8 },
        };
        let stationary_rivals = [Some(stationary)];
        let approaching_rivals = [Some(approaching)];
        let stationary_context = context(&stationary_rivals);
        let mut stationary_cache =
            ReturnForecastCache::new(resolve_return_threat(&stationary_context));
        let stationary_clearance = return_forecast_clearance(
            &stationary_context,
            &route,
            &mut CaptureScratch::default(),
            &mut stationary_cache,
        );
        let approaching_context = context(&approaching_rivals);
        let mut approaching_cache =
            ReturnForecastCache::new(resolve_return_threat(&approaching_context));
        let approaching_clearance = return_forecast_clearance(
            &approaching_context,
            &route,
            &mut CaptureScratch::default(),
            &mut approaching_cache,
        );
        assert!(approaching_clearance.clearance < stationary_clearance.clearance);
    }

    fn profiled_return(
        profile: &[f32],
        best_clearance: Option<f32>,
        mode: ReturnForecastMode,
    ) -> (super::forecast::RouteForecast, usize) {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let context = ReturnPlanContext {
            board: &board,
            territory: &territory,
            config: &config,
            id: CompetitorId(0),
            motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
            speed: config.player_speed,
            last_owned: Cell::new(0, 0),
            threat: None,
            rivals: &[],
            own_trail: None,
            competence: NpcCompetence { skill: 0.8 },
        };
        let route = NpcRoute::from_points(&[Vec2::new(4.0, 0.0)], RouteTarget::OwnedGround);
        let mut scratch = CaptureScratch::default();
        let mut calls = 0;
        let result = forecast_return_with_mode(
            &context,
            &route,
            &mut scratch,
            best_clearance,
            mode,
            |_, _| {
                let score = profile.get(calls).copied().unwrap_or(f32::INFINITY);
                calls += 1;
                score
            },
        );
        (result, calls)
    }

    #[test]
    fn return_branch_and_bound_only_rejects_strictly_dominated_profiles() {
        for (profile, should_prune) in [
            ([0.99_f32], false),
            ([1.01_f32], false),
            ([0.98_f32], true),
            ([f32::NAN], false),
            ([f32::INFINITY], false),
        ] {
            let (exhaustive, exhaustive_calls) =
                profiled_return(&profile, Some(1.0), ReturnForecastMode::Exhaustive);
            let (optimized, optimized_calls) =
                profiled_return(&profile, Some(1.0), ReturnForecastMode::BranchAndBound);
            assert!(exhaustive.is_reached());
            assert_eq!(optimized.is_reached(), !should_prune);
            if should_prune {
                assert!(optimized_calls < exhaustive_calls);
            } else {
                assert_eq!(optimized_calls, exhaustive_calls);
                assert_eq!(optimized.status, exhaustive.status);
                if !profile[0].is_nan() {
                    assert_eq!(optimized.clearance, exhaustive.clearance);
                }
            }
        }

        let (no_best, no_best_calls) =
            profiled_return(&[f32::INFINITY], None, ReturnForecastMode::BranchAndBound);
        assert!(no_best.is_reached());
        assert_eq!(no_best.clearance, f32::INFINITY);
        assert!(no_best_calls > 0);
    }

    #[test]
    fn return_branch_and_bound_preserves_invalid_and_zero_sample_routes() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let context = ReturnPlanContext {
            board: &board,
            territory: &territory,
            config: &config,
            id: CompetitorId(0),
            motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
            speed: config.player_speed,
            last_owned: Cell::new(0, 0),
            threat: None,
            rivals: &[],
            own_trail: None,
            competence: NpcCompetence { skill: 0.8 },
        };
        let invalid = NpcRoute::from_points(&[Vec2::NAN], RouteTarget::OwnedGround);
        let mut invalid_calls = 0;
        let invalid_result = forecast_return_with_mode(
            &context,
            &invalid,
            &mut CaptureScratch::default(),
            Some(1.0),
            ReturnForecastMode::BranchAndBound,
            |_, _| {
                invalid_calls += 1;
                1.0
            },
        );
        assert!(!invalid_result.is_viable());
        assert_eq!(invalid_calls, 0);

        let already_there = NpcRoute::from_points(&[Vec2::new(1.1, 0.0)], RouteTarget::OwnedGround);
        let mut zero_sample_calls = 0;
        let zero_sample_result = forecast_return_with_mode(
            &context,
            &already_there,
            &mut CaptureScratch::default(),
            Some(1.0),
            ReturnForecastMode::BranchAndBound,
            |_, _| {
                zero_sample_calls += 1;
                0.0
            },
        );
        assert!(zero_sample_result.is_reached());
        assert_eq!(zero_sample_calls, 0);
        assert_eq!(zero_sample_result.clearance, f32::INFINITY);
    }

    #[test]
    fn return_selection_matches_exhaustive_real_threat_forecasts() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let encounter = NpcEncounter {
            target: Some(HuntTarget::Rival(CompetitorId(1))),
            ..default()
        };
        let rivals = [Some(NpcVisibleRival {
            id: CompetitorId(1),
            relative: Vec2::X * 12.0,
            heading: -Vec2::X,
            speed: config.player_speed,
            ..default()
        })];
        let context = ReturnPlanContext {
            board: &board,
            territory: &territory,
            config: &config,
            id: CompetitorId(0),
            motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
            speed: config.player_speed,
            last_owned: Cell::new(0, 0),
            threat: Some(&encounter),
            rivals: &rivals,
            own_trail: None,
            competence: NpcCompetence { skill: 0.8 },
        };
        let routes = [
            NpcRoute::from_points(&[Vec2::new(4.0, 0.0)], RouteTarget::OwnedGround),
            NpcRoute::from_points(&[Vec2::new(0.0, 4.0)], RouteTarget::OwnedGround),
            NpcRoute::from_points(&[Vec2::new(-4.0, 0.0)], RouteTarget::OwnedGround),
        ];

        let select = |mode| {
            let mut best: Option<(f32, f32, NpcRoute)> = None;
            let mut scratch = CaptureScratch::default();
            let mut cache = ReturnForecastCache::new(resolve_return_threat(&context));
            for route in routes.iter().copied() {
                let result = forecast_return_with_mode(
                    &context,
                    &route,
                    &mut scratch,
                    best.as_ref().map(|(clearance, _, _)| *clearance),
                    mode,
                    |position, elapsed| {
                        cache
                            .threat_position(&context, elapsed)
                            .map_or(f32::INFINITY, |threat| position.distance(threat))
                    },
                );
                if !result.is_reached() {
                    continue;
                }
                if result.clearance == f32::INFINITY
                    || best.as_ref().is_none_or(|(clearance, eta, _)| {
                        result.clearance > *clearance + 0.01
                            || (result.clearance - *clearance).abs() <= 0.01
                                && result.elapsed < *eta
                    })
                {
                    best = Some((result.clearance, result.elapsed, route));
                }
            }
            best.map(|(_, _, route)| route)
        };

        assert_eq!(
            select(ReturnForecastMode::BranchAndBound),
            select(ReturnForecastMode::Exhaustive)
        );
    }
}
