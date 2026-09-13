use super::planner::{
    CapturePlanContext, CaptureRequest, CaptureScratch, ReturnPlanContext, committed_route_safe,
    plan_capture, plan_safe_return,
};
use super::*;
use crate::{
    ids::{CompetitorId, OwnerId},
    movement::{CompetitorMotion, advance_motion},
    trail::swept_trail_impact,
};
use bevy::prelude::*;

const ROAM_DISTANCE_MULTIPLIER: f32 = 2.0;
const RAID_LATE_ABORT_GRACE: f32 = 6.0;

fn policy_risk(policy: NpcPolicy) -> f32 {
    match policy {
        NpcPolicy::Builder(policy) => match policy.shape {
            BuilderShape::Fill => 0.28,
            BuilderShape::Seal => 0.22,
            BuilderShape::BroadSweep => 0.48,
            BuilderShape::Roamer => 0.18,
        },
        NpcPolicy::Hunter(policy) => match policy.target {
            HunterTarget::Trail => 0.34,
            HunterTarget::ExposedRival => 0.42,
            HunterTarget::Opportunistic => 0.30,
        },
        NpcPolicy::Raider(policy) => match policy.shape {
            RaidShape::Hook => 0.66,
            RaidShape::Wedge => 0.78,
        },
    }
}

fn choose_roamer_tactic(
    id: CompetitorId,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
    profile: NpcProfile,
) -> Option<NpcTactic> {
    let heading = observation.heading.normalize_or(Vec2::X);
    let side = match profile.policy {
        NpcPolicy::Builder(policy) => heading.perp() * policy.side.sign(),
        _ => heading.perp(),
    };
    let distance = (context.speed * ROAM_DISTANCE_MULTIPLIER)
        .max(context.board.cell_size * 4.0)
        .min(profile.competence.sensor_horizon());
    let directions = [
        heading,
        (heading + side * 0.75).normalize_or(heading),
        (heading - side * 0.75).normalize_or(heading),
        -heading,
    ];
    let margin = context.config.collision_radius * 2.0;
    directions.into_iter().find_map(|direction| {
        let desired = observation.position + direction * distance;
        let point = context
            .territory
            .arena_boundary()
            .project_inside(desired, margin)?;
        (point.distance(observation.position) >= context.board.cell_size
            && context.territory.owner_at(point) == OwnerId::UNCLAIMED)
            .then(|| {
                let route = NpcRoute::from_points(&[point], RouteTarget::OpenSpace);
                let mut tactic = NpcTactic::new(NpcTacticKind::Roam, route, context.tick);
                tactic.phase = TacticPhase::Travelling;
                tactic.left_owned = !context.territory.owns(observation.position, id);
                tactic
            })
    })
}

fn capture_context<'a>(
    context: &'a NpcTickContext<'a>,
    id: CompetitorId,
    observation: &NpcObservation,
    profile: NpcProfile,
    rivals: &'a [NpcVisibleRival],
    segments: &'a [NpcVisibleSegment],
) -> CapturePlanContext<'a> {
    CapturePlanContext {
        board: context.board,
        territory: context.territory,
        config: context.config,
        id,
        position: observation.position,
        heading: observation.heading,
        speed: context.speed,
        rank: context.rank,
        profile,
        rivals,
        segments,
        own_trail: context.own_trail,
    }
}

pub fn choose_builder_tactic(
    id: CompetitorId,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
    profile: NpcProfile,
    memory: &NpcEventMemory,
    scratch: &mut CaptureScratch,
) -> Option<NpcTactic> {
    if observation.trail_length > 0.0 || !observation.owns_current_cell {
        return Some(NpcTactic::new(
            NpcTacticKind::Return(ReturnReason::TrailLimit),
            plan_safe_return(&ReturnPlanContext {
                board: context.board,
                territory: context.territory,
                config: context.config,
                id,
                motion: CompetitorMotion::new(observation.position, observation.heading),
                speed: context.speed,
                last_owned: context.last_owned,
                threat: Some(&observation.encounter),
                rivals: &observation.rivals,
                own_trail: context.own_trail,
                competence: profile.competence,
            }),
            context.tick,
        ));
    }
    if matches!(
        profile.policy,
        NpcPolicy::Builder(BuilderPolicy {
            shape: BuilderShape::Roamer,
            ..
        })
    ) {
        return choose_roamer_tactic(id, observation, context, profile);
    }
    let revenge_segment = observation
        .segments
        .iter()
        .flatten()
        .find(|segment| !segment.own && memory.revenge_available(context.tick, segment.owner));
    let revenge_location = memory
        .recent
        .iter()
        .take(memory.recent_len as usize)
        .rev()
        .find(|event| {
            event
                .opponent
                .is_some_and(|opponent| memory.revenge_available(context.tick, opponent))
        })
        .map(|event| event.location)
        .filter(|point| point.is_finite() && *point != Vec2::ZERO);
    let purpose = if revenge_segment.is_some() || revenge_location.is_some() {
        CapturePurpose::SealGap
    } else {
        match profile.policy {
            NpcPolicy::Builder(policy) => match policy.shape {
                BuilderShape::Seal => CapturePurpose::SealGap,
                BuilderShape::BroadSweep | BuilderShape::Fill | BuilderShape::Roamer => {
                    CapturePurpose::FillFrontier
                }
            },
            _ => CapturePurpose::FillFrontier,
        }
    };
    let side = match profile.policy {
        NpcPolicy::Builder(policy) => policy.side,
        _ => TurnSide::Left,
    };
    let request = CaptureRequest {
        purpose,
        target: revenge_segment
            .map(|segment| segment.nearest_point)
            .or(revenge_location),
        target_owner: revenge_segment.map(|segment| segment.owner),
        preferred_side: side,
        max_risk: policy_risk(profile.policy),
        raid_shape: None,
    };
    let rival_storage: Vec<_> = observation.rivals.iter().flatten().copied().collect();
    plan_capture(
        &capture_context(
            context,
            id,
            observation,
            profile,
            &rival_storage,
            &observation
                .segments
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
        ),
        request,
        scratch,
    )
    .map(|plan| {
        let mut tactic = NpcTactic::new(
            NpcTacticKind::Capture(plan.purpose),
            plan.route,
            context.tick,
        );
        tactic.phase = TacticPhase::Travelling;
        tactic.left_owned = false;
        tactic
    })
}

const INTERCEPT_STEP: f32 = 1.0 / 30.0;
const INTERCEPT_MAX_STEPS: usize = 150;
const RETURN_FRONTIER_CAP: usize = 12;

fn normal_abort_trail_limit(profile: NpcProfile) -> f32 {
    8.0 + profile.competence.reaction_horizon() * 12.0
}

fn raid_abort_trail_limit(profile: NpcProfile, mistake: Option<NpcMistake>) -> f32 {
    normal_abort_trail_limit(profile)
        + if mistake == Some(NpcMistake::LateAbort) {
            RAID_LATE_ABORT_GRACE
        } else {
            0.0
        }
}

/// Find an intercept on a finite, still-active segment. The segment is static
/// evidence: it is not extrapolated from its tangent and it does not require
/// the rival body to be touched. Both the NPC and the rival return forecast use
/// the same authoritative finite-turn movement primitive as gameplay.
fn best_segment_intercept(
    segment: &NpcVisibleSegment,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
    rival_return_eta: f32,
) -> Option<(Vec2, f32)> {
    let segment_vector = segment.end - segment.start;
    let segment_length_squared = segment_vector.length_squared();
    if !segment.start.is_finite()
        || !segment.end.is_finite()
        || !segment_length_squared.is_finite()
        || segment_length_squared <= f32::EPSILON
    {
        return None;
    }

    let radius = context.config.collision_radius + context.config.trail_width * 0.5;
    let projection = segment.start
        + segment_vector
            * ((segment.nearest_point - segment.start).dot(segment_vector)
                / segment_length_squared)
                .clamp(0.0, 1.0);
    let mut targets = Vec::with_capacity(11);
    targets.push(projection);
    targets.push(segment.start);
    targets.push(segment.end);
    for sample in 1..=8 {
        targets.push(segment.start.lerp(segment.end, sample as f32 / 9.0));
    }

    let mut best: Option<(Vec2, f32)> = None;
    for target in targets {
        let Some(cut_eta) = static_segment_arrival_eta(
            observation.position,
            observation.heading,
            target,
            segment.start,
            segment.end,
            context,
            radius,
        ) else {
            continue;
        };
        // A cut only has tactical value while the exposed body is still
        // plausibly unable to re-enter its own ground. This ETA starts at the
        // observed rival body, never at the cut target.
        if cut_eta <= rival_return_eta + INTERCEPT_STEP {
            let candidate = (target, cut_eta);
            if best.is_none_or(|(_, eta)| cut_eta < eta) {
                best = Some(candidate);
            }
        }
    }
    best
}

fn static_segment_arrival_eta(
    position: Vec2,
    heading: Vec2,
    target: Vec2,
    segment_start: Vec2,
    segment_end: Vec2,
    context: &NpcTickContext<'_>,
    radius: f32,
) -> Option<f32> {
    let trail = [segment_start, segment_end];
    let mut motion = CompetitorMotion::new(position, heading);
    if point_segment_distance_for_intercept(position, segment_start, segment_end) <= radius {
        return Some(0.0);
    }
    for step in 0..INTERCEPT_MAX_STEPS {
        let previous = motion.position;
        let desired = (target - motion.position).try_normalize();
        advance_motion(
            &mut motion,
            desired,
            context.territory,
            context.config,
            context.speed,
            INTERCEPT_STEP,
        );
        let impact = swept_trail_impact(previous, motion.position, radius, &trail);
        if let Some(impact) = impact {
            return Some((step as f32 + impact) * INTERCEPT_STEP);
        }
        if point_segment_distance_for_intercept(motion.position, segment_start, segment_end)
            <= radius
        {
            return Some((step as f32 + 1.0) * INTERCEPT_STEP);
        }
        if motion.position.distance(target) <= radius * 0.5 {
            return None;
        }
    }
    None
}

fn hunter_segment_is_candidate(
    segment: NpcVisibleSegment,
    target: HunterTarget,
    observation: &NpcObservation,
    competence: NpcCompetence,
) -> bool {
    match target {
        HunterTarget::Trail => true,
        HunterTarget::ExposedRival => observation
            .rivals
            .iter()
            .flatten()
            .any(|rival| rival.id == segment.owner && rival.exposed),
        // Opportunistic hunters spend their pursuit budget only on nearby
        // trails, while still taking an exposed rival's trail at any sensed
        // distance. This makes opportunism a favorable-opening policy rather
        // than an alias for the unconditional trail hunter.
        HunterTarget::Opportunistic => {
            let nearby = segment.distance <= competence.sensor_horizon() * 0.5;
            nearby
                || observation
                    .rivals
                    .iter()
                    .flatten()
                    .any(|rival| rival.id == segment.owner && rival.exposed)
        }
    }
}

fn rival_home_reentry_eta_for_owner(
    owner: CompetitorId,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
    competence: NpcCompetence,
) -> f32 {
    observation
        .rivals
        .iter()
        .flatten()
        .find(|rival| rival.id == owner)
        .copied()
        .map_or(f32::INFINITY, |rival| {
            rival_home_reentry_eta(observation.position, rival, owner, context, competence)
        })
}

fn cached_rival_home_reentry_eta(
    owner: CompetitorId,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
    competence: NpcCompetence,
    cache: &mut Vec<(CompetitorId, f32)>,
) -> f32 {
    if let Some((_, eta)) = cache
        .iter()
        .find(|(cached_owner, _)| *cached_owner == owner)
    {
        return *eta;
    }
    let eta = rival_home_reentry_eta_for_owner(owner, observation, context, competence);
    cache.push((owner, eta));
    eta
}

fn rival_home_reentry_eta(
    observer_position: Vec2,
    rival: NpcVisibleRival,
    owner: CompetitorId,
    context: &NpcTickContext<'_>,
    competence: NpcCompetence,
) -> f32 {
    let position = observer_position + rival.relative;
    if context.territory.owns(position, owner) || !rival.speed.is_finite() || rival.speed <= 0.0 {
        return if context.territory.owns(position, owner) {
            0.0
        } else {
            f32::INFINITY
        };
    }
    let mut frontiers = Vec::with_capacity(RETURN_FRONTIER_CAP);
    context.territory.collect_owner_frontiers(
        position,
        owner,
        1.8 * competence.sensor_horizon(),
        &mut frontiers,
    );
    frontiers.sort_by(|a, b| {
        a.position
            .distance(position)
            .total_cmp(&b.position.distance(position))
    });
    frontiers.truncate(RETURN_FRONTIER_CAP);
    frontiers
        .into_iter()
        .filter_map(|frontier| {
            motion_eta_to_owned_frontier(
                CompetitorMotion::new(position, rival.heading),
                frontier.position,
                owner,
                context,
                rival.speed,
            )
        })
        .min_by(f32::total_cmp)
        .unwrap_or(f32::INFINITY)
}

fn motion_eta_to_owned_frontier(
    mut motion: CompetitorMotion,
    target: Vec2,
    owner: CompetitorId,
    context: &NpcTickContext<'_>,
    speed: f32,
) -> Option<f32> {
    for step in 0..INTERCEPT_MAX_STEPS {
        let previous = motion.position;
        let desired = (target - motion.position).try_normalize();
        advance_motion(
            &mut motion,
            desired,
            context.territory,
            context.config,
            speed,
            INTERCEPT_STEP,
        );
        if context.territory.owns(motion.position, owner) {
            return Some((step as f32 + 1.0) * INTERCEPT_STEP);
        }
        if motion.position.distance(previous) <= f32::EPSILON {
            return None;
        }
    }
    None
}

#[cfg(test)]
fn winning_segment_intercept(
    tactic: &NpcTactic,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
    competence: NpcCompetence,
) -> bool {
    let NpcTacticKind::Hunt(HuntTarget::Segment { owner, segment }) = tactic.kind else {
        return false;
    };
    // Segment indices are part of the selected target identity. A different
    // segment from the same owner may be reachable, but it cannot establish
    // that this hunt is winning.
    observation
        .segments
        .iter()
        .flatten()
        .find(|visible| !visible.own && visible.owner == owner && visible.segment == segment)
        .is_some_and(|visible| {
            let rival_return_eta =
                rival_home_reentry_eta_for_owner(visible.owner, observation, context, competence);
            best_segment_intercept(visible, observation, context, rival_return_eta).is_some()
        })
}

pub fn choose_hunter_tactic(
    _id: CompetitorId,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
    profile: NpcProfile,
    memory: &NpcEventMemory,
) -> Option<NpcTactic> {
    if context.tick < memory.pursuit_cooldown_until {
        return None;
    }
    let target_policy = match profile.policy {
        NpcPolicy::Hunter(policy) => policy.target,
        _ => HunterTarget::Opportunistic,
    };
    // Every segment from one rival shares the same return forecast. Keep the
    // cache local so it cannot outlive this observation/context pair.
    let mut rival_return_etas = Vec::with_capacity(NPC_VISIBLE_SEGMENT_CAP);
    let segment = observation
        .segments
        .iter()
        .flatten()
        .filter(|segment| !segment.own)
        .filter(|segment| {
            hunter_segment_is_candidate(**segment, target_policy, observation, profile.competence)
        })
        .find_map(|segment| {
            let rival_return_eta = cached_rival_home_reentry_eta(
                segment.owner,
                observation,
                context,
                profile.competence,
                &mut rival_return_etas,
            );
            best_segment_intercept(segment, observation, context, rival_return_eta)
                .map(|target| (*segment, target))
        });
    let (segment, (target, _)) = segment?;
    let mut tactic = NpcTactic::new(
        NpcTacticKind::Hunt(HuntTarget::Segment {
            owner: segment.owner,
            segment: segment.segment,
        }),
        NpcRoute::from_points(
            &[target],
            RouteTarget::Segment {
                owner: segment.owner,
                index: segment.segment,
            },
        ),
        context.tick,
    );
    // The selected segment has already passed the identical winning-intercept
    // forecast above, against this immutable observation and tick context.
    tactic.phase = TacticPhase::Committing;
    Some(tactic)
}

pub fn choose_raider_tactic(
    id: CompetitorId,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
    profile: NpcProfile,
    memory: &mut NpcEventMemory,
    scratch: &mut CaptureScratch,
) -> Option<NpcTactic> {
    if context.tick < memory.pursuit_cooldown_until {
        return None;
    }
    let policy = match profile.policy {
        NpcPolicy::Raider(policy) => policy,
        _ => return None,
    };
    let (owner, target, purpose) = match policy.objective {
        RaidObjective::TrailCut => {
            let segment = observation.segments.iter().flatten().find(|s| !s.own)?;
            (
                segment.owner,
                segment.nearest_point,
                CapturePurpose::CutTrail,
            )
        }
        RaidObjective::Leader | RaidObjective::WeakestBorder => {
            let mut rivals: Vec<_> = observation.rivals.iter().flatten().copied().collect();
            rivals.sort_by(|a, b| {
                let order = match policy.objective {
                    RaidObjective::Leader => b.territory_share.total_cmp(&a.territory_share),
                    RaidObjective::WeakestBorder => a.territory_share.total_cmp(&b.territory_share),
                    RaidObjective::TrailCut => std::cmp::Ordering::Equal,
                };
                order.then_with(|| a.id.cmp(&b.id))
            });
            let rival = rivals.into_iter().next()?;
            let mut frontiers = Vec::with_capacity(16);
            context.territory.collect_owner_frontiers(
                observation.position,
                rival.id,
                profile.competence.sensor_horizon(),
                &mut frontiers,
            );
            // The frontier is the entry identity, not the raid objective. The
            // outward normal points out of the owner's region, so step against
            // it to guarantee that the objective is an actual enemy-owned
            // point rather than merely a nearby boundary coordinate.
            let target = frontiers
                .into_iter()
                .filter_map(|frontier| {
                    [1.5_f32, 2.5, 3.5, 5.0]
                        .into_iter()
                        .map(|depth| {
                            frontier.position - frontier.outward.normalize_or(Vec2::Y) * depth
                        })
                        .find(|point| context.territory.owner_at(*point) == rival.id.owner())
                        .map(|point| (point.distance(observation.position), point))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, target)| target)?;
            (rival.id, target, CapturePurpose::RaidBorder)
        }
    };
    let rivals: Vec<_> = observation.rivals.iter().flatten().copied().collect();
    let request = CaptureRequest {
        purpose,
        target: Some(target),
        target_owner: Some(owner),
        preferred_side: match policy.shape {
            RaidShape::Hook => TurnSide::Left,
            RaidShape::Wedge => TurnSide::Right,
        },
        max_risk: policy_risk(profile.policy),
        raid_shape: Some(policy.shape),
    };
    let plan = plan_capture(
        &capture_context(
            context,
            id,
            observation,
            profile,
            &rivals,
            &observation
                .segments
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
        ),
        request,
        scratch,
    )?;
    if memory.revenge_available(context.tick, owner) {
        memory.use_revenge();
    }
    let mut tactic = NpcTactic::new(
        NpcTacticKind::Raid(RaidTarget::Border {
            owner,
            point: target,
        }),
        plan.route,
        context.tick,
    );
    // A border raid has an owned entry, an enemy interior leg, and a distinct
    // home re-entry; its bounded duration must cover those real waypoints,
    // rather than expiring at the generic short pursuit window.
    tactic.max_duration_ticks = 480;
    tactic.phase = TacticPhase::Committing;
    Some(tactic)
}

pub fn tactic_interrupted(
    tactic: &NpcTactic,
    observation: &NpcObservation,
    profile: NpcProfile,
    tick: u64,
) -> bool {
    if observation.protected || observation.edge_distance < 1.0 {
        return true;
    }
    if tick < tactic.next_interrupt_tick {
        return false;
    }
    match tactic.kind {
        NpcTacticKind::Return(_) => false,
        // A visible trail remains a valid static target even when its owner is
        // farther than the body's instantaneous distance heuristic. Only an
        // exposed-rival pursuit uses the short intercept timeout.
        NpcTacticKind::Hunt(HuntTarget::Segment { .. }) => observation.encounter.confidence < 0.12,
        NpcTacticKind::Hunt(HuntTarget::Rival(_)) => {
            observation.encounter.confidence < 0.12 || observation.encounter.intercept_time > 2.5
        }
        // Border raids have the same bounded trail budget as captures. A
        // LateAbort mistake adds a finite grace window instead of making the
        // raid abort sooner; arena and spawn safety above remain independent.
        NpcTacticKind::Raid(_) => {
            observation.trail_length > raid_abort_trail_limit(profile, tactic.mistake)
        }
        NpcTacticKind::Capture(_) => {
            observation.trail_length > normal_abort_trail_limit(profile)
                && tactic.mistake != Some(NpcMistake::LateAbort)
        }
        NpcTacticKind::Roam => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tactic_interrupted_with_context(
    tactic: &NpcTactic,
    observation: &NpcObservation,
    profile: NpcProfile,
    tick: u64,
    id: CompetitorId,
    motion: CompetitorMotion,
    context: &NpcTickContext<'_>,
    scratch: &mut CaptureScratch,
) -> bool {
    if observation.protected || observation.edge_distance < 1.0 {
        return true;
    }
    if tick < tactic.next_interrupt_tick {
        return false;
    }
    match tactic.kind {
        NpcTacticKind::Raid(_) => {
            if observation.trail_length <= raid_abort_trail_limit(profile, tactic.mistake) {
                return false;
            }
            // A raid may spend its authored budget only after it is committed
            // and only when the same fixed-tick forecast proves target contact
            // and a safe owned re-entry. Otherwise this remains an abort.
            tactic.phase != TacticPhase::Committing
                || !committed_route_safe(
                    &super::planner::ForecastContext {
                        board: context.board,
                        territory: context.territory,
                        config: context.config,
                        id,
                        motion,
                        speed: context.speed,
                        own_trail: context.own_trail,
                        competence: profile.competence,
                    },
                    &tactic.route,
                    tactic.route_index as usize,
                    if tactic.mistake == Some(NpcMistake::OvercommitReturn) {
                        0.35
                    } else {
                        1.15
                    },
                    scratch,
                )
        }
        _ => tactic_interrupted(tactic, observation, profile, tick),
    }
}

pub fn route_direction(tactic: &NpcTactic, position: Vec2, heading: Vec2) -> Vec2 {
    let point = tactic
        .route
        .active(tactic.route_index.min(tactic.route.count.saturating_sub(1)));
    let direction = point.map_or(heading, |point| point - position);
    let direction = match tactic.mistake {
        Some(NpcMistake::MisreadIntercept) if tactic.route_index == 0 => direction - heading * 0.75,
        Some(NpcMistake::PoorSideChoice) if tactic.route_index == 0 => {
            direction + direction.perp() * 0.12
        }
        _ => direction,
    };
    direction.normalize_or(heading)
}

fn point_segment_distance_for_intercept(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let segment = end - start;
    if segment.length_squared() <= f32::EPSILON {
        return point.distance(start);
    }
    let t = ((point - start).dot(segment) / segment.length_squared()).clamp(0.0, 1.0);
    point.distance(start + segment * t)
}

#[cfg(test)]
mod tests;
