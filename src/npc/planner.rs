//! Bounded NPC route planning.
//!
//! The planner is split by responsibility: capture geometry and candidate
//! selection, return/raid validation, and the fixed-tick motion forecast.

use super::*;
use crate::{board::TrailSegmentRef, territory_map::OwnerFrontier};

mod capture;
mod forecast;
mod geometry;
mod return_plan;

pub use capture::{CapturePlan, CapturePlanContext, CaptureRequest, plan_capture};
pub(crate) use forecast::{ForecastContext, ForecastGoal, ForecastRequest, forecast_route};
pub use geometry::segment_has_ownership;
pub(crate) use return_plan::committed_route_safe;
pub use return_plan::{ReturnPlanContext, plan_safe_return};

pub(crate) const FRONTIER_SAMPLE_CAP: usize = 72;
pub(crate) const ROUTE_SAMPLE_CAP: usize = 16;
pub(crate) const FORECAST_ROUTE_STEP_CAP: usize = 480;
pub(crate) const FORECAST_CANDIDATE_CAP: usize = 16;
pub(crate) const WAYPOINT_ARRIVAL_DISTANCE: f32 = 1.15;
pub(crate) const TRAIL_QUERY_RADIUS: f32 = 2.5;

/// Reusable bounded allocations shared by capture and return planning.
pub struct CaptureScratch {
    pub frontiers: Vec<OwnerFrontier>,
    pub candidates: Vec<NpcRoute>,
    pub trail_refs: Vec<TrailSegmentRef>,
    pub trail_candidates: Vec<usize>,
    /// Accepted motion samples; scoring happens only after route safety passes.
    pub(crate) forecast_samples: Vec<(Vec2, f32)>,
}

impl Default for CaptureScratch {
    fn default() -> Self {
        Self {
            frontiers: Vec::with_capacity(FRONTIER_SAMPLE_CAP),
            candidates: Vec::with_capacity(FORECAST_CANDIDATE_CAP),
            trail_refs: Vec::with_capacity(NPC_RELEVANT_TRAIL_SEGMENT_CAP),
            trail_candidates: Vec::with_capacity(NPC_RELEVANT_TRAIL_SEGMENT_CAP),
            forecast_samples: Vec::with_capacity(FORECAST_ROUTE_STEP_CAP),
        }
    }
}
