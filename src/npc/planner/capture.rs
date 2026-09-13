use super::*;
use crate::{
    board::{BoardGrid, point_segment_distance},
    config::GameConfig,
    movement::{CompetitorMotion, predict_position},
    territory_map::TerritoryMap,
    trail::ActiveTrail,
};
use bevy::prelude::*;

pub struct CapturePlanContext<'a> {
    pub board: &'a BoardGrid,
    pub territory: &'a TerritoryMap,
    pub config: &'a GameConfig,
    pub id: CompetitorId,
    pub position: Vec2,
    pub heading: Vec2,
    pub speed: f32,
    pub rank: u8,
    pub profile: NpcProfile,
    pub rivals: &'a [NpcVisibleRival],
    pub segments: &'a [NpcVisibleSegment],
    pub own_trail: Option<&'a ActiveTrail>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureRequest {
    pub purpose: CapturePurpose,
    pub target: Option<Vec2>,
    pub target_owner: Option<CompetitorId>,
    pub preferred_side: TurnSide,
    /// Authored policy risk/size. This is never derived from competence.
    pub max_risk: f32,
    pub raid_shape: Option<RaidShape>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CapturePlan {
    pub route: NpcRoute,
    pub purpose: CapturePurpose,
    pub estimated_area: f32,
    pub exit_distance: f32,
    pub return_clearance: f32,
}

/// Rival motion depends on elapsed forecast time, not the candidate route.
/// Cache it for this planning call only; observations cannot become stale
/// across ticks. Each successful candidate visits samples in fixed-tick order.
#[derive(Default)]
struct RivalForecastCache {
    samples: Vec<Vec<Vec2>>,
}

impl RivalForecastCache {
    fn positions(
        &mut self,
        context: &CapturePlanContext<'_>,
        tick: usize,
        elapsed: f32,
    ) -> &[Vec2] {
        if tick == self.samples.len() {
            self.samples.push(
                context
                    .rivals
                    .iter()
                    .filter(|rival| rival.speed.is_finite())
                    .map(|rival| {
                        predict_position(
                            CompetitorMotion::new(context.position + rival.relative, rival.heading),
                            Some(rival.heading),
                            context.territory,
                            context.config,
                            rival.speed,
                            elapsed,
                        )
                    })
                    .collect(),
            );
        }
        &self.samples[tick]
    }
}

pub fn plan_capture(
    context: &CapturePlanContext<'_>,
    request: CaptureRequest,
    scratch: &mut CaptureScratch,
) -> Option<CapturePlan> {
    scratch.frontiers.clear();
    scratch.candidates.clear();
    scratch.trail_refs.clear();
    let horizon = context.profile.competence.sensor_horizon();
    context.territory.collect_owner_frontiers(
        context.position,
        context.id,
        horizon * 1.8,
        &mut scratch.frontiers,
    );
    scratch.frontiers.truncate(FRONTIER_SAMPLE_CAP);
    if scratch.frontiers.len() < 2 {
        return None;
    }

    // Do bounded, cheap route construction and geometry checks first. The
    // frontier set is intentionally larger than the forecast budget, so rank
    // it before invoking finite-turn motion and swept-trail checks.
    for (exit_index, exit) in scratch.frontiers.iter().enumerate() {
        // A capture must close on a distinct observed frontier. Returning to the
        // same anchor creates a no-op out-and-back loop and cannot seal a pocket.
        let reentry = scratch
            .frontiers
            .iter()
            .enumerate()
            .filter(|(index, frontier)| {
                *index != exit_index
                    && frontier.position.distance(exit.position) >= context.board.cell_size
            })
            .min_by(|(_, a), (_, b)| {
                let a_key = request
                    .target
                    .map_or(a.position.distance(exit.position), |target| {
                        a.position.distance(target)
                    });
                let b_key = request
                    .target
                    .map_or(b.position.distance(exit.position), |target| {
                        b.position.distance(target)
                    });
                a_key
                    .total_cmp(&b_key)
                    .then_with(|| a.position.x.total_cmp(&b.position.x))
                    .then_with(|| a.position.y.total_cmp(&b.position.y))
            })
            .map(|(_, frontier)| *frontier);
        let Some(reentry) = reentry else {
            continue;
        };

        let entry_inside = exit.position
            + (-exit.outward).normalize_or(Vec2::Y) * context.board.cell_size.max(1.0);
        let reentry_inside = reentry.position
            + (-reentry.outward).normalize_or(Vec2::Y) * context.board.cell_size.max(1.0);
        let target = request.target.unwrap_or_else(|| {
            exit.position + exit.outward * purpose_depth(request.purpose, request.max_risk)
        });
        let side = exit.outward.perp() * request.preferred_side.sign();
        let extent = purpose_width(request.purpose, request.max_risk);
        let outward =
            exit.position + exit.outward * purpose_depth(request.purpose, request.max_risk);
        let shape = match request.purpose {
            CapturePurpose::FillFrontier => vec![
                entry_inside,
                exit.position,
                outward,
                outward + side * extent,
                reentry.position,
            ],
            CapturePurpose::SealGap => vec![
                entry_inside,
                exit.position,
                target.lerp(exit.position, 0.5),
                target,
                reentry.position,
            ],
            CapturePurpose::CutTrail => vec![
                entry_inside,
                exit.position,
                target,
                target + side * extent.min(5.0),
                reentry.position,
            ],
            CapturePurpose::RaidBorder => match request.raid_shape.unwrap_or(RaidShape::Hook) {
                RaidShape::Hook => vec![
                    entry_inside,
                    exit.position,
                    target.lerp(exit.position, 0.3),
                    target,
                    reentry.position,
                ],
                RaidShape::Wedge => vec![
                    entry_inside,
                    exit.position,
                    target + side * extent,
                    target,
                    reentry_inside,
                    reentry.position,
                ],
            },
        };
        let route_target = request
            .target_owner
            .map_or(RouteTarget::OwnedGround, |owner| {
                RouteTarget::EnemyBorder(owner)
            });
        let route = NpcRoute::from_points(&shape, route_target);
        // Reject raids that never traverse the requested owner's actual cells
        // before doing the comparatively expensive motion/trail forecast.
        if (request.purpose == CapturePurpose::RaidBorder
            && !geometry::route_crosses_owner(
                context.territory,
                route.target,
                &route,
                context.board.cell_size,
            ))
            || !geometry::valid_route_geometry(
                context.territory,
                context.config.collision_radius,
                &route,
                context.position,
            )
            || !route
                .final_point()
                .is_some_and(|point| context.territory.owns(point, context.id))
        {
            continue;
        }
        scratch.candidates.push(route);
    }

    scratch.candidates.sort_unstable_by(|a, b| {
        cheap_route_rank(context, request, a)
            .total_cmp(&cheap_route_rank(context, request, b))
            .reverse()
            .then_with(|| route_tie_breaker(a, b))
    });
    scratch.candidates.truncate(FORECAST_CANDIDATE_CAP);

    let forecast_context = ForecastContext {
        board: context.board,
        territory: context.territory,
        config: context.config,
        id: context.id,
        motion: CompetitorMotion::new(context.position, context.heading),
        speed: context.speed,
        own_trail: context.own_trail,
        competence: context.profile.competence,
    };
    let mut rival_forecasts = RivalForecastCache::default();
    let mut best: Option<CapturePlan> = None;
    for index in 0..scratch.candidates.len() {
        let route = scratch.candidates[index];
        let mut candidate = CapturePlan {
            route,
            purpose: request.purpose,
            estimated_area: purpose_depth(request.purpose, request.max_risk)
                * purpose_width(request.purpose, request.max_risk),
            exit_distance: context.position.distance(route.points[0]),
            return_clearance: horizon,
        };
        // Every authored candidate has multiple waypoints, hence an accepted
        // forecast scores at least one movement sample. Scoring starts at the
        // sensor horizon and only decreases. If even that upper bound cannot
        // beat the incumbent, no motion/safety work can change the selection.
        if horizon.is_finite()
            && best
                .as_ref()
                .is_some_and(|old| !capture_plan_is_better(&candidate, old))
        {
            continue;
        }
        // The clearance forecast already validates the same motion and trail
        // when starting without an active trail. Only existing history needs
        // the separate safety pass (the clearance model deliberately omits it).
        if context.own_trail.is_some() {
            let result = forecast_route(
                ForecastRequest {
                    context: forecast_context,
                    route: &route,
                    route_index: 0,
                    max_ticks: FORECAST_ROUTE_STEP_CAP,
                    goal: ForecastGoal::ReachEndpoint {
                        waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
                    },
                },
                scratch,
                |_, _| f32::INFINITY,
            );
            if !result.is_reached() {
                continue;
            }
        }
        let result = route_threat_clearance(context, &route, scratch, &mut rival_forecasts);
        if context.own_trail.is_none() && !result.is_reached() {
            continue;
        }
        let clearance = if result.is_reached() {
            result.clearance
        } else {
            0.0
        };
        // Unsafe legs are rejected before geometry/area is considered. A route
        // with no predicted clearance is not a risky choice; it is invalid.
        if clearance < context.config.collision_radius * 2.0
            && (request.max_risk < 0.45 || request.purpose == CapturePurpose::RaidBorder)
        {
            continue;
        }
        candidate.return_clearance = clearance;
        let better = best
            .as_ref()
            .is_none_or(|old| capture_plan_is_better(&candidate, old));
        if better {
            best = Some(candidate);
        }
    }
    best
}

fn capture_plan_is_better(candidate: &CapturePlan, old: &CapturePlan) -> bool {
    if candidate.purpose == CapturePurpose::RaidBorder {
        candidate.exit_distance < old.exit_distance - 0.01
            || (candidate.exit_distance - old.exit_distance).abs() <= 0.01
                && candidate.return_clearance > old.return_clearance + 0.01
    } else {
        candidate.return_clearance > old.return_clearance + 0.01
            || (candidate.return_clearance - old.return_clearance).abs() <= 0.01
                && candidate.estimated_area > old.estimated_area
            || (candidate.return_clearance - old.return_clearance).abs() <= 0.01
                && (candidate.estimated_area - old.estimated_area).abs() <= 0.01
                && candidate.exit_distance < old.exit_distance
    }
}

fn route_threat_clearance(
    context: &CapturePlanContext<'_>,
    route: &NpcRoute,
    scratch: &mut CaptureScratch,
    rivals: &mut RivalForecastCache,
) -> super::forecast::RouteForecast {
    let forecast = ForecastContext {
        board: context.board,
        territory: context.territory,
        config: context.config,
        id: context.id,
        motion: CompetitorMotion::new(context.position, context.heading),
        speed: context.speed,
        own_trail: None,
        competence: context.profile.competence,
    };
    let mut scored_tick = 0;
    forecast_route(
        ForecastRequest {
            context: forecast,
            route,
            route_index: 0,
            max_ticks: FORECAST_ROUTE_STEP_CAP,
            goal: ForecastGoal::ReachEndpoint {
                waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
            },
        },
        scratch,
        |position, elapsed| {
            let mut minimum = context.profile.competence.sensor_horizon();
            for segment in context.segments.iter().filter(|segment| !segment.own) {
                minimum = minimum.min(point_segment_distance(position, segment.start, segment.end));
            }
            for &predicted in rivals.positions(context, scored_tick, elapsed) {
                minimum = minimum.min(position.distance(predicted));
            }
            scored_tick += 1;
            minimum
        },
    )
}

fn cheap_route_rank(
    context: &CapturePlanContext<'_>,
    request: CaptureRequest,
    route: &NpcRoute,
) -> f32 {
    let mut previous = context.position;
    let mut length = 0.0;
    let mut target_distance = f32::INFINITY;
    for point in route.points.iter().take(route.count as usize) {
        length += previous.distance(*point);
        target_distance =
            target_distance.min(request.target.map_or(0.0, |target| point.distance(target)));
        previous = *point;
    }
    let exit_distance = route
        .active(0)
        .map_or(f32::INFINITY, |point| context.position.distance(point));
    if request.purpose == CapturePurpose::RaidBorder {
        -(exit_distance + length * 0.05 + target_distance * 0.25)
    } else {
        -(length + exit_distance * 0.1)
    }
}

fn route_tie_breaker(a: &NpcRoute, b: &NpcRoute) -> std::cmp::Ordering {
    a.count.cmp(&b.count).then_with(|| {
        a.points
            .iter()
            .zip(b.points.iter())
            .map(|(a, b)| a.x.total_cmp(&b.x).then_with(|| a.y.total_cmp(&b.y)))
            .find(|ordering| *ordering != std::cmp::Ordering::Equal)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn purpose_depth(purpose: CapturePurpose, risk: f32) -> f32 {
    match purpose {
        CapturePurpose::FillFrontier => 5.0 + risk.clamp(0.0, 1.0) * 5.0,
        CapturePurpose::SealGap => 4.0,
        CapturePurpose::CutTrail => 6.0,
        CapturePurpose::RaidBorder => 8.0 + risk.clamp(0.0, 1.0) * 8.0,
    }
}

fn purpose_width(purpose: CapturePurpose, risk: f32) -> f32 {
    let risk = risk.clamp(0.0, 1.0);
    match purpose {
        CapturePurpose::FillFrontier => 4.0 + risk,
        CapturePurpose::SealGap => 3.0,
        CapturePurpose::CutTrail => 2.5,
        CapturePurpose::RaidBorder => 5.0 + risk * 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{board::BoardGrid, config::GameConfig, territory_map::TerritoryMap};

    fn setup() -> (BoardGrid, TerritoryMap) {
        let board = BoardGrid::generate(91, 2, &GameConfig::default());
        let mut territory = TerritoryMap::from_board(&board);
        territory.seed_owner(Vec2::ZERO, 10.0, CompetitorId(0));
        (board, territory)
    }

    fn profile() -> NpcProfile {
        NpcProfile {
            policy: NpcPolicy::Builder(BuilderPolicy {
                shape: BuilderShape::Fill,
                side: TurnSide::Left,
            }),
            competence: NpcCompetence { skill: 0.8 },
        }
    }

    #[test]
    fn clearance_upper_bound_never_prunes_a_winning_candidate() {
        let mut pruned = 0;
        for purpose in [
            CapturePurpose::FillFrontier,
            CapturePurpose::SealGap,
            CapturePurpose::CutTrail,
            CapturePurpose::RaidBorder,
        ] {
            for old_clearance in [0.0, 4.0, 10.0, 20.0] {
                let old = CapturePlan {
                    route: NpcRoute::default(),
                    purpose,
                    estimated_area: 25.0,
                    exit_distance: 2.0,
                    return_clearance: old_clearance,
                };
                for exit_distance in [1.0, 1.989, 1.99, 1.991, 2.0, 2.009, 2.01, 2.011, 3.0] {
                    let upper = CapturePlan {
                        exit_distance,
                        return_clearance: 20.0,
                        ..old
                    };
                    if capture_plan_is_better(&upper, &old) {
                        continue;
                    }
                    pruned += 1;
                    for clearance in [
                        0.0,
                        old_clearance - 0.011,
                        old_clearance - 0.01,
                        old_clearance - 0.009,
                        old_clearance,
                        old_clearance + 0.009,
                        old_clearance + 0.01,
                        old_clearance + 0.011,
                        20.0,
                    ] {
                        if !(0.0..=20.0).contains(&clearance) {
                            continue;
                        }
                        assert!(!capture_plan_is_better(
                            &CapturePlan {
                                return_clearance: clearance,
                                ..upper
                            },
                            &old
                        ));
                    }
                }
            }
        }
        assert!(pruned > 0);
    }

    #[test]
    fn rival_cache_reuses_exact_predictions_across_candidate_lengths() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let rivals = [NpcVisibleRival {
            id: CompetitorId(1),
            relative: Vec2::X * 20.0,
            heading: -Vec2::X,
            speed: config.player_speed,
            ..default()
        }];
        let context = CapturePlanContext {
            board: &board,
            territory: &territory,
            config: &config,
            id: CompetitorId(0),
            position: Vec2::ZERO,
            heading: Vec2::X,
            speed: config.player_speed,
            rank: 1,
            profile: profile(),
            rivals: &rivals,
            segments: &[],
            own_trail: None,
        };
        let mut cache = RivalForecastCache::default();
        for candidate_length in [20, 8, 30] {
            for tick in 0..candidate_length {
                let elapsed = (tick + 1) as f32 * config.fixed_delta_seconds();
                let expected = predict_position(
                    CompetitorMotion::new(rivals[0].relative, rivals[0].heading),
                    Some(rivals[0].heading),
                    &territory,
                    &config,
                    rivals[0].speed,
                    elapsed,
                );
                assert_eq!(cache.positions(&context, tick, elapsed), &[expected]);
            }
            assert_eq!(cache.samples.len(), candidate_length.max(20));
        }
    }

    #[test]
    fn raid_route_enters_target_owner_before_distinct_reentry() {
        let config = GameConfig::default();
        let board = BoardGrid::generate(91, 2, &config);
        let mut territory = TerritoryMap::from_board(&board);
        territory.seed_owner(Vec2::new(-12.0, 0.0), 3.5, CompetitorId(0));
        territory.seed_owner(Vec2::new(12.0, 0.0), 3.5, CompetitorId(1));
        let profile = NpcProfile {
            policy: NpcPolicy::Raider(RaiderPolicy {
                objective: RaidObjective::Leader,
                shape: RaidShape::Hook,
            }),
            competence: NpcCompetence { skill: 0.8 },
        };
        let mut scratch = CaptureScratch::default();
        let plan = plan_capture(
            &CapturePlanContext {
                board: &board,
                territory: &territory,
                config: &config,
                id: CompetitorId(0),
                position: Vec2::new(-12.0, 0.0),
                heading: Vec2::X,
                speed: config.player_speed,
                rank: 2,
                profile,
                rivals: &[],
                segments: &[],
                own_trail: None,
            },
            CaptureRequest {
                purpose: CapturePurpose::RaidBorder,
                target: Some(Vec2::new(10.0, 0.0)),
                target_owner: Some(CompetitorId(1)),
                preferred_side: TurnSide::Left,
                max_risk: 0.66,
                raid_shape: Some(RaidShape::Hook),
            },
            &mut scratch,
        )
        .expect("a nearby enemy lobe should support a border raid");
        assert_eq!(plan.route.target, RouteTarget::EnemyBorder(CompetitorId(1)));
        assert!(territory.owns(plan.route.final_point().unwrap(), CompetitorId(0)));
        assert!(
            plan.route.points[..plan.route.count as usize]
                .iter()
                .any(|point| territory.owner_at(*point) == CompetitorId(1).owner())
        );
        assert!(
            plan.route.points[0].distance(plan.route.final_point().unwrap()) >= config.cell_size
        );
    }

    #[test]
    fn purpose_routes_are_bounded_and_return_owned() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let mut scratch = CaptureScratch::default();
        let plan = plan_capture(
            &CapturePlanContext {
                board: &board,
                territory: &territory,
                config: &config,
                id: CompetitorId(0),
                position: Vec2::ZERO,
                heading: Vec2::X,
                speed: config.player_speed,
                rank: 1,
                profile: profile(),
                rivals: &[],
                segments: &[],
                own_trail: None,
            },
            CaptureRequest {
                purpose: CapturePurpose::FillFrontier,
                target: None,
                target_owner: None,
                preferred_side: TurnSide::Left,
                max_risk: 0.6,
                raid_shape: None,
            },
            &mut scratch,
        )
        .unwrap();
        assert!(plan.route.count >= 2 && plan.route.count <= 8);
        assert!(territory.owns(plan.route.final_point().unwrap(), CompetitorId(0)));
        assert_ne!(plan.route.points[0], plan.route.final_point().unwrap());
    }
}
