use crate::ids::CompetitorId;

use super::{
    CapturePlan, NpcAction, NpcDecision, NpcDecisionFrame, NpcEvent, NpcMatchMemory, NpcTraits,
    capture_risk_budget,
};

pub trait NpcBrain: Send + Sync + 'static {
    fn decide(
        &mut self,
        frame: &NpcDecisionFrame,
        traits: NpcTraits,
        memory: &NpcMatchMemory,
    ) -> NpcDecision;
    fn on_event(&mut self, _event: NpcEvent) {}
}

pub struct UtilityTactician {
    id: CompetitorId,
}

impl UtilityTactician {
    pub fn new(id: CompetitorId) -> Self {
        Self { id }
    }
}

impl NpcBrain for UtilityTactician {
    fn decide(
        &mut self,
        frame: &NpcDecisionFrame,
        traits: NpcTraits,
        memory: &NpcMatchMemory,
    ) -> NpcDecision {
        let nearest_rival = frame
            .rivals
            .iter()
            .flatten()
            .min_by(|a, b| a.distance.total_cmp(&b.distance));
        let risk_budget = frame
            .proposed_capture
            .map_or(memory.active_plan_risk, |plan| plan.risk_budget)
            .max(capture_risk_budget(
                frame.visible_rank,
                traits,
                memory,
                frame.rivals.iter().flatten(),
            ));

        if frame.protected {
            return decision(NpcAction::Continue, frame.heading, 0.05, 0.20, None);
        }
        if frame.edge_distance < 2.0 {
            return decision(
                NpcAction::ReturnHome,
                frame.inward_direction,
                0.02,
                0.12,
                None,
            );
        }

        if !frame.owns_current_cell || frame.trail_length > 0.0 {
            let home_distance = frame
                .home
                .map_or(f32::INFINITY, |home| home.distance(frame.position));
            let interception_margin = nearest_rival.map_or(f32::INFINITY, |rival| {
                let learned = memory.opponents[rival.id.index()].aggression;
                rival.distance / 8.0 - learned * 0.35
            });
            let return_time = home_distance / 8.0;
            let threatened = interception_margin
                < return_time * (1.35 - risk_budget * 0.55 - traits.composure * 0.20);
            let excursion_limit = if frame.active_waypoint.is_some() {
                f32::INFINITY
            } else {
                9.0 + risk_budget * 18.0
            };
            let can_abort_plan = frame.active_waypoint.is_none() || memory.waypoint_index >= 2;
            if (threatened && can_abort_plan)
                || frame.trail_length > excursion_limit
                || frame.active_waypoint.is_none()
            {
                return decision(
                    NpcAction::ReturnHome,
                    frame
                        .home
                        .map_or(frame.inward_direction, |home| home - frame.position),
                    risk_budget,
                    0.12,
                    None,
                );
            }
            let waypoint = frame.active_waypoint.expect("checked capture waypoint");
            return decision(
                NpcAction::FollowCapturePlan,
                waypoint - frame.position,
                risk_budget,
                0.20 + traits.commitment * 0.25,
                None,
            );
        }

        if let Some(waypoint) = frame.active_waypoint {
            return decision(
                NpcAction::FollowCapturePlan,
                waypoint - frame.position,
                risk_budget,
                0.20 + traits.commitment * 0.25,
                None,
            );
        }

        if let Some(trail) = frame.trails.iter().flatten().find(|trail| !trail.own)
            && traits.aggression * (0.7 + memory.frustration * 0.3) > 0.58
            && trail.distance < 5.0 + traits.aggression * 8.0
        {
            return decision(
                NpcAction::HuntTrail,
                trail.relative,
                risk_budget,
                0.18,
                None,
            );
        }

        if frame.visible_rank > 3
            && traits.aggression > 0.62
            && let Some(leader) = frame
                .rivals
                .iter()
                .flatten()
                .filter(|rival| rival.exposed)
                .max_by(|a, b| a.territory_share.total_cmp(&b.territory_share))
        {
            return decision(
                NpcAction::StealLeader,
                leader.relative,
                risk_budget,
                0.20,
                None,
            );
        }

        if let Some(plan) = frame.proposed_capture {
            let identity_delay = f32::from(self.id.0 % 3) * 0.025;
            return decision(
                NpcAction::BeginCapture,
                plan.waypoints[0] - frame.position,
                plan.risk_budget,
                0.18 + traits.commitment * 0.25 + identity_delay,
                Some(plan),
            );
        }

        decision(
            NpcAction::Explore,
            frame.quiet_direction,
            risk_budget,
            0.18,
            None,
        )
    }
}

pub struct LegacyWanderer {
    side: f32,
}

impl LegacyWanderer {
    pub fn new(id: CompetitorId) -> Self {
        Self {
            side: if id.0.is_multiple_of(2) { 1.0 } else { -1.0 },
        }
    }
}

impl NpcBrain for LegacyWanderer {
    fn decide(
        &mut self,
        frame: &NpcDecisionFrame,
        _traits: NpcTraits,
        memory: &NpcMatchMemory,
    ) -> NpcDecision {
        if frame.edge_distance < 2.0 {
            return decision(
                NpcAction::ReturnHome,
                frame.inward_direction,
                0.35,
                0.25,
                None,
            );
        }
        if (!frame.owns_current_cell || frame.trail_length > 0.0) && frame.trail_length > 11.0 {
            return decision(
                NpcAction::ReturnHome,
                frame
                    .home
                    .map_or(frame.inward_direction, |home| home - frame.position),
                0.55,
                0.35,
                None,
            );
        }
        if let Some(waypoint) = frame.active_waypoint {
            return decision(
                NpcAction::FollowCapturePlan,
                waypoint - frame.position,
                memory.active_plan_risk,
                0.55,
                None,
            );
        }
        if let Some(plan) = frame.proposed_capture {
            return decision(
                NpcAction::BeginCapture,
                plan.waypoints[0] - frame.position,
                plan.risk_budget,
                0.55,
                Some(plan),
            );
        }
        let curve = frame.heading.perp() * self.side * 0.35;
        decision(NpcAction::Explore, frame.heading + curve, 0.65, 0.55, None)
    }

    fn on_event(&mut self, event: NpcEvent) {
        if matches!(event, NpcEvent::OwnCapture { .. } | NpcEvent::Died { .. }) {
            self.side = -self.side;
        }
    }
}

fn decision(
    action: NpcAction,
    direction: bevy::math::Vec2,
    risk_budget: f32,
    commitment_seconds: f32,
    capture_plan: Option<CapturePlan>,
) -> NpcDecision {
    NpcDecision {
        action,
        desired_direction: direction.normalize_or(bevy::math::Vec2::Y),
        risk_budget: risk_budget.clamp(0.0, 1.0),
        commitment_seconds,
        capture_plan,
    }
}
