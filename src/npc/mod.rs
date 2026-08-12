//! Runtime-light NPC sensing, capture planning, decision-making, and roster generation.
//!
//! Brains only receive [`NpcDecisionFrame`].  Board and entity queries stay in
//! the match integration layer, which makes the local-information boundary
//! explicit and testable.

mod brain;
mod model;
mod planner;
mod roster;
mod sensing;

pub use brain::{LegacyWanderer, NpcBrain, UtilityTactician};
pub use model::*;
pub use planner::{CapturePlanContext, capture_risk_budget, propose_capture_plan};
pub use roster::{NpcRosterEntry, generate_npc_roster};
pub use sensing::build_decision_frame;

use bevy::prelude::*;

use crate::{ids::CompetitorId, input::SteeringIntent};

#[derive(Component)]
pub struct NpcController {
    pub brain: Box<dyn NpcBrain>,
    pub brain_kind: NpcBrainKind,
    pub traits: NpcTraits,
    pub memory: NpcMatchMemory,
    pub think_remaining: f32,
    pub last_decision: NpcDecision,
    pub steering_error: f32,
    pub steering_error_target: f32,
    pub error_epoch: u32,
    pub safety_override: bool,
}

impl NpcController {
    pub fn from_roster(id: CompetitorId, entry: &NpcRosterEntry, slot: usize) -> Self {
        let brain: Box<dyn NpcBrain> = match entry.brain_kind {
            NpcBrainKind::Tactical => Box::new(UtilityTactician::new(id)),
            NpcBrainKind::LegacyWanderer => Box::new(LegacyWanderer::new(id)),
        };
        let heading = Vec2::Y;
        Self {
            brain,
            brain_kind: entry.brain_kind,
            traits: entry.traits,
            memory: NpcMatchMemory::new(entry.traits.adaptability >= 0.001),
            // Fixed-tick staggering avoids a single large thought batch.
            think_remaining: slot as f32 / 12.0 / entry.traits.think_hz(),
            last_decision: NpcDecision::continue_in(heading),
            steering_error: 0.0,
            steering_error_target: 0.0,
            error_epoch: 0,
            safety_override: false,
        }
    }

    pub fn on_event(&mut self, event: NpcEvent) {
        self.memory.on_event(event, self.traits.adaptability);
        self.brain.on_event(event);
    }

    pub fn prepare_thought(&mut self, position: Vec2, delta_seconds: f32) {
        self.memory.advance(delta_seconds);
        self.memory.advance_waypoint_if_reached(position, 1.15);
    }

    pub fn decide(&mut self, frame: &NpcDecisionFrame) {
        let mut decision = self.brain.decide(frame, self.traits, &self.memory);
        self.safety_override = false;
        if !decision.desired_direction.is_finite()
            || decision.desired_direction.length_squared() < 0.01
        {
            decision = NpcDecision::continue_in(frame.heading);
            self.safety_override = true;
        }
        if frame.edge_distance < 0.9 && decision.desired_direction.dot(frame.inward_direction) < 0.1
        {
            decision.desired_direction = frame.inward_direction;
            self.safety_override = true;
        }

        let urgent_return = decision.action == NpcAction::ReturnHome
            && (!frame.owns_current_cell || frame.edge_distance < 2.0);
        if self.memory.commitment_remaining > 0.0
            && decision.action != self.memory.current_action
            && !self.safety_override
            && !urgent_return
        {
            decision = self.last_decision;
        }

        if let Some(plan) = decision.capture_plan {
            self.memory.begin_capture(plan);
            decision.action = NpcAction::FollowCapturePlan;
            decision.desired_direction =
                (plan.waypoints[0] - frame.position).normalize_or(frame.heading);
            decision.capture_plan = None;
        }
        let action_changed = decision.action != self.memory.current_action;
        self.memory.current_action = decision.action;
        if action_changed {
            self.memory.commitment_remaining = decision
                .commitment_seconds
                .max(self.traits.minimum_commitment());
        }
        self.last_decision = decision;
        self.memory.decision_counter = self.memory.decision_counter.wrapping_add(1);
    }

    pub fn write_steering(&self, steering: &mut SteeringIntent) {
        let (sin, cos) = self.steering_error.sin_cos();
        let direction = self.last_decision.desired_direction;
        steering.desired_direction = Vec2::new(
            direction.x * cos - direction.y * sin,
            direction.x * sin + direction.y * cos,
        )
        .normalize_or(Vec2::Y);
        steering.magnitude = 1.0;
        steering.source = crate::input::ControlSource::Npc;
    }
}

pub fn update_colored_error(controller: &mut NpcController, id: CompetitorId, match_time: f32) {
    let epoch = (match_time / 1.75).floor() as u32;
    if epoch != controller.error_epoch {
        controller.error_epoch = epoch;
        let mixed = mix64(
            u64::from(id.0) ^ u64::from(epoch).rotate_left(21) ^ controller.memory.decision_counter,
        );
        let unit = ((mixed >> 40) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0;
        controller.steering_error_target = unit * controller.traits.error_radians();
    }
    // Low-pass interpolation makes error drift instead of jumping or sharing a
    // synchronized sinusoid across the roster.
    controller.steering_error +=
        (controller.steering_error_target - controller.steering_error) * 0.22;
}

pub(crate) fn mix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedBrain(NpcDecision);

    impl NpcBrain for FixedBrain {
        fn decide(
            &mut self,
            _frame: &NpcDecisionFrame,
            _traits: NpcTraits,
            _memory: &NpcMatchMemory,
        ) -> NpcDecision {
            self.0
        }
    }

    #[test]
    fn repeating_an_action_does_not_perpetually_renew_commitment() {
        let traits = NpcTraits {
            skill: 0.5,
            aggression: 0.5,
            greed: 0.5,
            exploration: 0.5,
            composure: 0.5,
            adaptability: 0.5,
            commitment: 0.5,
            turning_bias: 0.0,
        };
        let repeated = NpcDecision {
            action: NpcAction::Explore,
            desired_direction: Vec2::X,
            risk_budget: 0.5,
            commitment_seconds: 0.8,
            capture_plan: None,
        };
        let mut controller = NpcController {
            brain: Box::new(FixedBrain(repeated)),
            brain_kind: NpcBrainKind::Tactical,
            traits,
            memory: NpcMatchMemory::new(true),
            think_remaining: 0.0,
            last_decision: repeated,
            steering_error: 0.0,
            steering_error_target: 0.0,
            error_epoch: 0,
            safety_override: false,
        };
        controller.memory.current_action = NpcAction::Explore;
        controller.memory.commitment_remaining = 0.05;
        controller.decide(&decision_frame());
        assert_eq!(controller.memory.commitment_remaining, 0.05);
    }

    fn decision_frame() -> NpcDecisionFrame {
        NpcDecisionFrame {
            position: Vec2::ZERO,
            heading: Vec2::Y,
            protected: false,
            owns_current_cell: true,
            trail_length: 0.0,
            visible_rank: 1,
            edge_distance: 20.0,
            inward_direction: -Vec2::Y,
            home: None,
            active_waypoint: None,
            proposed_capture: None,
            quiet_direction: Vec2::X,
            rivals: [None; 3],
            trails: [None; 2],
        }
    }
}
