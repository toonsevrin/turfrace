//! Concrete, encounter-first NPC policies and bounded local perception.
pub mod lab;
mod model;

mod planner;
mod policy;
mod roster;
mod sensing;

pub use model::*;
pub use planner::{
    CapturePlan, CapturePlanContext, CaptureRequest, CaptureScratch, ReturnPlanContext,
    plan_capture, plan_safe_return, segment_has_ownership,
};
pub use policy::{
    choose_builder_tactic, choose_hunter_tactic, choose_raider_tactic, route_direction,
    tactic_interrupted,
};
pub use roster::{NpcRosterEntry, generate_npc_roster};
pub(crate) use sensing::collect_bounded_nearby_trail_segments;
pub use sensing::{NpcTrailSnapshot, build_observation};

use crate::{ids::CompetitorId, match_game::SteeringIntent, movement::CompetitorMotion};
use bevy::prelude::*;

use planner::{ForecastContext, ForecastGoal, ForecastRequest, forecast_route};
use policy::tactic_interrupted_with_context;

#[derive(Component)]
pub struct NpcController {
    pub id: CompetitorId,
    pub profile: NpcProfile,
    pub tactic: Option<NpcTactic>,
    pub memory: NpcEventMemory,
    pub think_remaining: f32,
    pub steering_error: f32,
    pub steering_error_target: f32,
    pub error_epoch: u32,
    pub safety_override: bool,
    pub mistake_cooldown_tick: u64,
    pub tactic_sequence: u64,
    pub last_reason: &'static str,
    pub last_steering: Vec2,
}
impl NpcController {
    pub fn from_roster(id: CompetitorId, entry: &NpcRosterEntry, slot: usize) -> Self {
        Self {
            id,
            profile: entry.profile,
            tactic: None,
            memory: NpcEventMemory::new(),
            think_remaining: slot as f32 / 12.0 / entry.profile.competence.think_hz(),
            steering_error: 0.0,
            steering_error_target: 0.0,
            error_epoch: 0,
            safety_override: false,
            mistake_cooldown_tick: 0,
            tactic_sequence: 0,
            last_reason: "spawn",
            last_steering: Vec2::Y,
        }
    }
    pub fn on_event(&mut self, event: NpcEvent, tick: u64) {
        self.memory.on_event(event, tick);
        if self
            .tactic
            .as_ref()
            .is_some_and(|t| event_relevant_to_tactic(event, t.kind))
            || matches!(event, NpcEvent::Spawned | NpcEvent::Died { .. })
        {
            self.tactic = None;
        }
    }
    pub fn tick(
        &mut self,
        observation: &NpcObservation,
        motion: CompetitorMotion,
        context: &NpcTickContext<'_>,
        scratch: &mut CaptureScratch,
    ) {
        self.safety_override = false;
        self.memory
            .advance(1.0 / self.profile.competence.think_hz());
        if let Some(tactic) = self.tactic.as_mut() {
            if !observation.owns_current_cell {
                tactic.left_owned = true;
            }
            // A return can be selected while the NPC is already safely on its
            // own ground (for example after an encounter disappears). Treat
            // that as the completed side of the same invariant, but never do
            // so while an active trail still exists: trail re-entry must still
            // be observed after leaving owned ground.
            if return_is_already_safe_on_owned_ground(tactic, observation, context) {
                tactic.left_owned = true;
            }
        }
        if self.tactic.as_ref().is_some_and(|t| {
            let threshold = if t.mistake == Some(NpcMistake::OvercommitReturn) {
                0.35
            } else {
                1.15
            };
            t.route_index.saturating_add(1) < t.route.count
                && t.route
                    .active(t.route_index)
                    .is_some_and(|p| p.distance(observation.position) < threshold)
        }) && let Some(t) = self.tactic.as_mut()
        {
            t.route_index = t.route_index.saturating_add(1);
            t.phase = TacticPhase::Travelling;
        }
        let completed = self.tactic.filter(|t| {
            let at_final = t.route.count > 0 && t.route_index.saturating_add(1) >= t.route.count;
            let observed_reentry = t.left_owned && observation.owns_current_cell;
            at_final
                && match t.kind {
                    NpcTacticKind::Return(_) | NpcTacticKind::Capture(_) => observed_reentry,
                    NpcTacticKind::Hunt(_) | NpcTacticKind::Raid(_) => false,
                    NpcTacticKind::Roam => true,
                }
        });
        if let Some(completed) = completed {
            let opponent = tactic_opponent(completed.kind);
            self.finish_tactic(
                completed,
                EncounterOutcome::Completed,
                context.tick,
                observation.position,
            );
            self.tactic = None;
            self.last_reason = "observed tactic completion";
            let _ = opponent;
        }
        let known_ambusher = observation
            .encounter
            .target
            .map(|target| match target {
                HuntTarget::Segment { owner, .. } | HuntTarget::Rival(owner) => owner,
            })
            .is_some_and(|owner| self.memory.opponents[owner.index()].threat > 0.6);
        // A promising hunt never overrides the authoritative swept self-trail
        // check. Winning an interception is not permission to collide with our
        // own active trail on the way there.
        let self_collision_risk = imminent_self_collision(
            observation,
            self.id,
            motion,
            context,
            self.tactic.as_ref(),
            self.profile.competence,
            scratch,
        );
        let hard_return = !observation.protected
            && (observation.edge_distance < 1.0
                || known_ambusher
                || self_collision_risk
                || observation.trail_length > 0.0
                    && !observation.owns_current_cell
                    && observation.edge_distance < 2.0);
        if let Some(tactic) = self.tactic.as_mut() {
            refresh_hunt_target(tactic, observation, context);
            refresh_raid_target(tactic, observation, context);
        }
        let target_loss_reason = self.tactic.as_ref().and_then(|t| {
            if tactic_timed_out(t, context.tick) {
                Some("timed out return")
            } else if !tactic_target_visible(t, observation, context) {
                Some("lost target return")
            } else if tactic_is_losing(t, observation, context.tick) {
                Some("losing encounter return")
            } else {
                None
            }
        });
        let target_gone = target_loss_reason.is_some();
        let emergency_replan = self.tactic.as_ref().is_some_and(|t| {
            matches!(t.kind, NpcTacticKind::Return(_))
                && matches!(t.route.target, RouteTarget::EmergencyReturn)
                && !observation.owns_current_cell
                && t.route
                    .final_point()
                    .is_some_and(|point| point.distance(observation.position) <= 1.15)
        });
        if hard_return
            || emergency_replan
            || target_gone
            || self.tactic.as_ref().is_some_and(|t| {
                tactic_interrupted_with_context(
                    t,
                    observation,
                    self.profile,
                    context.tick,
                    self.id,
                    motion,
                    context,
                    scratch,
                )
            })
        {
            if let Some(abandoned) = self.tactic {
                self.finish_tactic(
                    abandoned,
                    EncounterOutcome::Abandoned,
                    context.tick,
                    observation.position,
                );
            }
            let needs_return =
                hard_return || observation.trail_length > 0.0 || !observation.owns_current_cell;
            if needs_return {
                let route = plan_safe_return(&ReturnPlanContext {
                    board: context.board,
                    territory: context.territory,
                    config: context.config,
                    id: self.id,
                    motion,
                    speed: context.speed,
                    last_owned: context.last_owned,
                    threat: Some(&observation.encounter),
                    rivals: &observation.rivals,
                    own_trail: context.own_trail,
                    competence: self.profile.competence,
                });
                let mut tactic = NpcTactic::new(
                    NpcTacticKind::Return(if hard_return && observation.edge_distance < 1.0 {
                        ReturnReason::Safety
                    } else {
                        ReturnReason::Threat
                    }),
                    route,
                    context.tick,
                );
                tactic.phase = TacticPhase::Aborting;
                self.tactic = Some(tactic);
                self.last_reason = if self_collision_risk {
                    "self-collision return"
                } else if known_ambusher {
                    "remembered ambusher return"
                } else if hard_return {
                    "arena safety return"
                } else if let Some(reason) = target_loss_reason {
                    reason
                } else if emergency_replan {
                    "emergency replan"
                } else {
                    "tactic interruption return"
                };
                self.safety_override = true;
            } else {
                // A gone/losing pursuit on owned ground is abandoned, not
                // converted into a fake return that can never observe reentry.
                self.tactic = None;
                self.last_reason = "pursuit abandoned";
            }
        }
        if self.tactic.is_none() && !target_gone {
            let selected = match self.profile.policy {
                NpcPolicy::Builder(_) => choose_builder_tactic(
                    self.id,
                    observation,
                    context,
                    self.profile,
                    &self.memory,
                    scratch,
                ),
                NpcPolicy::Hunter(_) => {
                    choose_hunter_tactic(self.id, observation, context, self.profile, &self.memory)
                }
                NpcPolicy::Raider(_) => choose_raider_tactic(
                    self.id,
                    observation,
                    context,
                    self.profile,
                    &mut self.memory,
                    scratch,
                ),
            };
            self.tactic = selected;
            if let Some(tactic) = self.tactic {
                self.last_reason = tactic_reason(tactic.kind);
                if matches!(
                    tactic.kind,
                    NpcTacticKind::Capture(_) | NpcTacticKind::Raid(_)
                ) {
                    self.memory.planned_capture_area =
                        tactic.route.count.saturating_sub(1) as f32 * 8.0;
                }
            }
            if self.tactic.is_some() {
                self.tactic_sequence = self.tactic_sequence.wrapping_add(1);
                self.select_mistake(context.tick, observation);
            }
        }
        let mut direction = self.tactic.as_ref().map_or(observation.heading, |t| {
            route_direction(t, observation.position, observation.heading)
        });
        if !direction.is_finite() || direction.length_squared() < 0.01 {
            direction = observation.heading;
            self.safety_override = true;
        }
        if observation.edge_distance < 0.9 && direction.dot(observation.inward_direction) < 0.1 {
            direction = observation.inward_direction;
            self.safety_override = true;
        }
        self.memory.decision_counter = self.memory.decision_counter.wrapping_add(1);
        self.last_steering = direction.normalize_or(observation.heading);
    }
    fn finish_tactic(
        &mut self,
        tactic: NpcTactic,
        outcome: EncounterOutcome,
        tick: u64,
        location: Vec2,
    ) {
        let opponent = tactic_opponent(tactic.kind);
        self.memory.record_encounter(
            opponent,
            outcome,
            tick,
            location,
            tactic.encounter.confidence,
        );
        if matches!(tactic.kind, NpcTacticKind::Hunt(_) | NpcTacticKind::Raid(_)) {
            self.memory.pursuit_cooldown_until = tick.saturating_add(90);
        }
        if let Some(mistake) = tactic.mistake {
            self.memory.last_failure = Some(FailureMemory {
                tick,
                mistake,
                location,
            });
        }
    }
    fn select_mistake(&mut self, tick: u64, _observation: &NpcObservation) {
        if tick < self.mistake_cooldown_tick || self.profile.competence.skill > 0.94 {
            return;
        }
        let plausible = self.tactic.as_ref().is_some_and(|t| {
            matches!(
                t.kind,
                NpcTacticKind::Capture(_)
                    | NpcTacticKind::Hunt(_)
                    | NpcTacticKind::Raid(_)
                    | NpcTacticKind::Return(_)
            )
        });
        let seed = self.id.0 as u64 ^ self.tactic_sequence.rotate_left(23);
        let roll = mix64(seed ^ tick.rotate_left(17)) as f32 / u64::MAX as f32;
        if plausible
            && roll < self.profile.competence.mistake_chance() * if tick < 1 { 0.5 } else { 1.0 }
        {
            let mistake = match self.tactic.as_ref().map(|t| t.kind) {
                Some(NpcTacticKind::Hunt(_)) => NpcMistake::MisreadIntercept,
                Some(NpcTacticKind::Return(_)) => NpcMistake::OvercommitReturn,
                Some(NpcTacticKind::Capture(_)) => NpcMistake::PoorSideChoice,
                // LateAbort is deliberately assigned only to raids, where
                // tactic_interrupted has a matching late-trail escape rule.
                Some(NpcTacticKind::Raid(_)) => NpcMistake::LateAbort,
                Some(NpcTacticKind::Roam) | None => return,
            };
            if let Some(tactic) = self.tactic.as_mut() {
                tactic.mistake = Some(mistake);
            }
            // Failure is recorded only once the tactic has an outcome. The
            // mistake remains attached to the tactic while it is in flight.
            self.mistake_cooldown_tick = tick.saturating_add(600);
            self.last_reason = "situational mistake";
        }
    }
    pub fn write_steering(&self, steering: &mut SteeringIntent) {
        steering.desired_direction = self.last_steering.normalize_or(Vec2::Y);
        steering.magnitude = 1.0;
        steering.source = crate::match_game::ControlSource::Npc;
    }
}
fn return_is_already_safe_on_owned_ground(
    tactic: &NpcTactic,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
) -> bool {
    matches!(tactic.kind, NpcTacticKind::Return(_))
        && matches!(tactic.route.target, RouteTarget::OwnedGround)
        && observation.owns_current_cell
        && observation.trail_length <= 0.0
        && context.own_trail.is_none()
}

#[allow(clippy::too_many_arguments)]
fn imminent_self_collision(
    observation: &NpcObservation,
    id: CompetitorId,
    motion: CompetitorMotion,
    context: &NpcTickContext<'_>,
    tactic: Option<&NpcTactic>,
    competence: NpcCompetence,
    scratch: &mut CaptureScratch,
) -> bool {
    let Some(_trail) = context.own_trail else {
        return false;
    };
    let route = tactic.map_or_else(
        || {
            NpcRoute::from_points(
                &[observation.position + observation.heading.normalize_or(Vec2::Y) * 100.0],
                RouteTarget::OpenSpace,
            )
        },
        |tactic| tactic.route,
    );
    let route_index = tactic.map_or(0, |tactic| tactic.route_index as usize);
    let threshold = tactic.map_or(1.15, |tactic| {
        if tactic.mistake == Some(NpcMistake::OvercommitReturn) {
            0.35
        } else {
            1.15
        }
    });
    // The planner kernel also appends synthetic future trail points, so this
    // guard sees the same exclusion window as detect_trail_collisions.
    let result = forecast_route(
        ForecastRequest {
            context: ForecastContext {
                board: context.board,
                territory: context.territory,
                config: context.config,
                id,
                motion,
                speed: context.speed,
                own_trail: context.own_trail,
                competence,
            },
            route: &route,
            route_index,
            max_ticks: 4,
            goal: ForecastGoal::SafeHorizon {
                waypoint_threshold: threshold,
            },
        },
        scratch,
        |_, _| f32::INFINITY,
    );
    !result.is_viable()
}

fn event_relevant_to_tactic(event: NpcEvent, kind: NpcTacticKind) -> bool {
    let opponent = tactic_opponent(kind);
    match event {
        NpcEvent::OwnCapture { .. } => matches!(
            kind,
            NpcTacticKind::Return(_) | NpcTacticKind::Capture(_) | NpcTacticKind::Raid(_)
        ),
        NpcEvent::CreditedKill { victim } => opponent == Some(victim),
        NpcEvent::TerritoryStolen { by, .. } => opponent == Some(by),
        NpcEvent::EncounterCompleted { opponent: other }
        | NpcEvent::EncounterAbandoned { opponent: other } => other == opponent,
        NpcEvent::Spawned | NpcEvent::Died { .. } => true,
    }
}

fn tactic_timed_out(tactic: &NpcTactic, tick: u64) -> bool {
    tick.saturating_sub(tactic.started_tick) >= u64::from(tactic.max_duration_ticks)
}

fn tactic_target_visible(
    tactic: &NpcTactic,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
) -> bool {
    match tactic.kind {
        NpcTacticKind::Hunt(HuntTarget::Segment { owner, .. }) => {
            // Segment indices are historical. Once the bounded sensing window
            // advances, the originally selected static segment may no longer
            // be listed even though the active trail still exists. Retain the
            // hunt while that owner's exposed trail remains observable.
            observation
                .segments
                .iter()
                .flatten()
                .any(|visible| !visible.own && visible.owner == owner)
        }
        NpcTacticKind::Raid(RaidTarget::Border { owner, point }) => {
            context.territory.owner_at(point) == owner.owner()
        }
        NpcTacticKind::Raid(RaidTarget::Leader(owner)) => observation
            .rivals
            .iter()
            .flatten()
            .any(|rival| rival.id == owner),
        NpcTacticKind::Capture(_) => match tactic.route.target {
            RouteTarget::EnemyBorder(owner) => tactic
                .route
                .points
                .iter()
                .take(tactic.route.count as usize)
                .any(|point| context.territory.owner_at(*point) == owner.owner()),
            _ => true,
        },
        // Emergency routes are deliberately persistent. Losing the original
        // encounter must not abandon the only hard-safety route; completion
        // still requires observing owned re-entry in `tick`.
        NpcTacticKind::Return(_) => true,
        _ => true,
    }
}

fn tactic_is_losing(tactic: &NpcTactic, observation: &NpcObservation, tick: u64) -> bool {
    if !matches!(tactic.kind, NpcTacticKind::Hunt(_) | NpcTacticKind::Raid(_))
        || tick < tactic.next_interrupt_tick
    {
        return false;
    }
    let target_owner = tactic_opponent(tactic.kind);
    // An enemy trail is a cut opportunity, not a moving threat. Retreat
    // from an approaching third-party body while exposed, not its static ink.
    let approaching_rival = matches!(tactic.kind, NpcTacticKind::Raid(_))
        && observation.trail_length > 0.0
        && observation.rivals.iter().flatten().any(|rival| {
            Some(rival.id) != target_owner
                && rival.distance < 3.0
                && rival.speed > 0.0
                && rival.heading.dot(-rival.relative) > 0.0
        });
    approaching_rival
        || (matches!(
            tactic.kind,
            NpcTacticKind::Hunt(HuntTarget::Segment { .. })
                | NpcTacticKind::Hunt(HuntTarget::Rival(_))
        ) && (observation.encounter.confidence < 0.12
            || matches!(tactic.kind, NpcTacticKind::Hunt(HuntTarget::Rival(_)))
                && observation.encounter.intercept_time > 2.5))
}

fn refresh_hunt_target(
    tactic: &mut NpcTactic,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
) {
    let NpcTacticKind::Hunt(HuntTarget::Segment { owner, segment }) = tactic.kind else {
        return;
    };
    let Some(visible) = observation
        .segments
        .iter()
        .flatten()
        .find(|visible| !visible.own && visible.owner == owner && visible.segment == segment)
    else {
        return;
    };
    let rival_speed = observation
        .rivals
        .iter()
        .flatten()
        .find(|rival| rival.id == owner)
        .map_or(context.speed, |rival| rival.speed.max(0.0));
    let segment = visible.end - visible.start;
    let length_squared = segment.length_squared();
    let projected = if length_squared > f32::EPSILON {
        ((visible.nearest_point - visible.start).dot(segment) / length_squared
            + rival_speed * 0.2 / length_squared)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    let target = visible.start.lerp(visible.end, projected);
    if target.is_finite() {
        tactic.route.points[0] = target;
        tactic.encounter = observation.encounter;
    }
}

fn refresh_raid_target(
    tactic: &mut NpcTactic,
    observation: &NpcObservation,
    context: &NpcTickContext<'_>,
) {
    let NpcTacticKind::Raid(RaidTarget::Border { owner, point }) = tactic.kind else {
        return;
    };
    if context.territory.owner_at(point) == owner.owner() {
        tactic.encounter = observation.encounter;
        return;
    }
    let mut frontiers = Vec::with_capacity(16);
    context.territory.collect_owner_frontiers(
        observation.position,
        owner,
        observation.speed.max(1.0) * 3.0 + 16.0,
        &mut frontiers,
    );
    let Some(frontier) = frontiers.into_iter().min_by(|a, b| {
        a.position
            .distance(observation.position)
            .total_cmp(&b.position.distance(observation.position))
    }) else {
        return;
    };
    if frontier.position.distance(point) > context.speed * 0.1 {
        tactic.kind = NpcTacticKind::Raid(RaidTarget::Border {
            owner,
            point: frontier.position,
        });
        for route_point in tactic
            .route
            .points
            .iter_mut()
            .take(tactic.route.count as usize)
        {
            if route_point.distance(point) < 0.01 {
                *route_point = frontier.position;
            }
        }
    }
    tactic.encounter = observation.encounter;
}

fn tactic_reason(kind: NpcTacticKind) -> &'static str {
    match kind {
        NpcTacticKind::Return(_) => "return route",
        NpcTacticKind::Capture(_) => "purpose capture",
        NpcTacticKind::Hunt(_) => "segment intercept",
        NpcTacticKind::Raid(_) => "enemy border raid",
        NpcTacticKind::Roam => "roam",
    }
}

fn tactic_opponent(kind: NpcTacticKind) -> Option<CompetitorId> {
    match kind {
        NpcTacticKind::Hunt(HuntTarget::Segment { owner, .. })
        | NpcTacticKind::Hunt(HuntTarget::Rival(owner))
        | NpcTacticKind::Raid(RaidTarget::Border { owner, .. })
        | NpcTacticKind::Raid(RaidTarget::Leader(owner)) => Some(owner),
        _ => None,
    }
}

pub(crate) fn mix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        board::BoardGrid, config::GameConfig, territory_map::TerritoryMap, trail::ActiveTrail,
    };

    fn observation() -> NpcObservation {
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
    fn raid_retreat_requires_an_approaching_body_not_static_enemy_ink() {
        let mut observation = observation();
        observation.trail_length = 5.0;
        observation.segments[0] = Some(NpcVisibleSegment {
            owner: CompetitorId(2),
            distance: 1.0,
            own: false,
            ..default()
        });
        let tactic = NpcTactic::new(
            NpcTacticKind::Raid(RaidTarget::Leader(CompetitorId(1))),
            NpcRoute::from_points(&[Vec2::X], RouteTarget::EnemyBorder(CompetitorId(1))),
            0,
        );
        assert!(!tactic_is_losing(&tactic, &observation, 20));
        observation.rivals[0] = Some(NpcVisibleRival {
            id: CompetitorId(2),
            relative: Vec2::X,
            distance: 1.0,
            heading: Vec2::X,
            speed: 8.0,
            exposed: true,
            territory_share: 0.1,
        });
        assert!(!tactic_is_losing(&tactic, &observation, 20));
        observation.rivals[0].as_mut().unwrap().heading = -Vec2::X;
        assert!(tactic_is_losing(&tactic, &observation, 20));
    }

    #[test]
    fn emergency_return_is_not_abandoned_when_encounter_target_is_gone() {
        let config = GameConfig::default();
        let board = BoardGrid::generate(91, 2, &config);
        let territory = TerritoryMap::from_board(&board);
        let observation = observation();
        let context = NpcTickContext {
            board: &board,
            territory: &territory,
            config: &config,
            rank: 1,
            tick: 0,
            speed: config.player_speed,
            last_owned: crate::board::Cell::new(0, 0),
            own_trail: None,
        };
        let tactic = NpcTactic::new(
            NpcTacticKind::Return(ReturnReason::Safety),
            NpcRoute::from_points(&[Vec2::X], RouteTarget::EmergencyReturn),
            0,
        );
        assert!(tactic_target_visible(&tactic, &observation, &context));
    }

    #[test]
    fn safe_owned_return_is_ready_without_masking_active_trail_reentry() {
        let config = GameConfig::default();
        let board = BoardGrid::generate(91, 2, &config);
        let territory = TerritoryMap::from_board(&board);
        let route = NpcRoute::from_points(&[Vec2::ZERO], RouteTarget::OwnedGround);
        let tactic = NpcTactic::new(NpcTacticKind::Return(ReturnReason::Threat), route, 0);
        let observation = observation();
        let context = NpcTickContext {
            board: &board,
            territory: &territory,
            config: &config,
            rank: 1,
            tick: 0,
            speed: config.player_speed,
            last_owned: crate::board::Cell::new(0, 0),
            own_trail: None,
        };
        assert!(return_is_already_safe_on_owned_ground(
            &tactic,
            &observation,
            &context,
        ));

        let mut trail = ActiveTrail::new(
            CompetitorId(0),
            crate::board::Cell::new(0, 0),
            Vec2::ZERO,
            Vec2::X,
        );
        trail.append_exact(Vec2::X);
        let context_with_trail = NpcTickContext {
            own_trail: Some(&trail),
            ..context
        };
        assert!(!return_is_already_safe_on_owned_ground(
            &tactic,
            &observation,
            &context_with_trail,
        ));
    }
}
