use super::*;
use crate::{
    board::BoardGrid,
    config::GameConfig,
    movement::{CompetitorMotion, predict_motion},
    territory_map::TerritoryMap,
    trail::{ActiveTrail, TrailSegmentAccessor, swept_self_active_trail_impact},
};

/// Small borrowed environment shared by every fixed-tick route forecast.
#[derive(Clone, Copy)]
pub(crate) struct ForecastContext<'a> {
    pub board: &'a BoardGrid,
    pub territory: &'a TerritoryMap,
    pub config: &'a GameConfig,
    pub id: CompetitorId,
    pub motion: CompetitorMotion,
    pub speed: f32,
    pub own_trail: Option<&'a ActiveTrail>,
    pub competence: NpcCompetence,
}

/// A forecast trail keeps the committed polyline borrowed and stores only
/// points sampled during this bounded forecast. Its logical segment indices
/// continue directly after the borrowed prefix, as they do after appending to
/// an `ActiveTrail`.
const FORECAST_TRAIL_CHUNK_SEGMENTS: usize = 16;

/// A chunk is deliberately monotone: an old bound may retain stale area, but
/// it can never lose area when the forecast's moving head is replaced.
#[derive(Clone, Copy)]
enum ForecastChunkBounds {
    Empty,
    Finite { min: Vec2, max: Vec2 },
    Invalid,
}

struct ForecastTrail<'a> {
    prefix: Option<&'a ActiveTrail>,
    anchor: Vec2,
    points: Vec<Vec2>,
    length: f32,
    last_sample_heading: Vec2,
    head: Vec2,
    recent_prefix: usize,
    chunks: Vec<ForecastChunkBounds>,
}

impl<'a> ForecastTrail<'a> {
    fn from_active(prefix: &'a ActiveTrail, capacity: usize) -> Self {
        let segment_count = prefix.segment_count();
        let first_relevant = segment_count.saturating_sub(NPC_RELEVANT_TRAIL_SEGMENT_CAP);
        let recent_prefix =
            first_relevant / FORECAST_TRAIL_CHUNK_SEGMENTS * FORECAST_TRAIL_CHUNK_SEGMENTS;
        let mut trail = Self {
            prefix: Some(prefix),
            anchor: prefix.points.last().copied().unwrap_or(prefix.head),
            points: Vec::with_capacity(capacity),
            length: prefix.length,
            last_sample_heading: prefix.last_sample_heading,
            head: prefix.head,
            recent_prefix,
            chunks: Vec::new(),
        };
        trail.ensure_chunk_for(segment_count.saturating_add(capacity).saturating_sub(1));
        for index in recent_prefix..segment_count {
            trail.expand_chunk(index);
        }
        trail
    }

    fn new(boundary: Vec2, heading: Vec2, capacity: usize) -> Self {
        let mut trail = Self {
            prefix: None,
            anchor: boundary,
            points: Vec::with_capacity(capacity),
            length: 0.0,
            last_sample_heading: heading,
            head: boundary,
            recent_prefix: 0,
            chunks: Vec::new(),
        };
        trail.ensure_chunk_for(capacity.saturating_sub(1));
        trail
    }

    fn append(
        &mut self,
        point: Vec2,
        heading: Vec2,
        distance_threshold: f32,
        angle_threshold: f32,
    ) -> bool {
        let old_last = self.segment_count().checked_sub(1);
        let moved = self.head.distance(point);
        self.length += moved;
        self.head = point;
        let last_sample = self.points.last().copied().unwrap_or(self.anchor);
        let moved_since_sample = last_sample.distance(point);
        let turned = self.last_sample_heading.angle_to(heading).abs();
        if moved_since_sample + 1e-5 < distance_threshold && turned + 1e-5 < angle_threshold {
            self.refresh_changed_chunks(old_last);
            return false;
        }
        self.points.push(point);
        self.last_sample_heading = heading;
        self.refresh_changed_chunks(old_last);
        true
    }

    fn refresh_changed_chunks(&mut self, old_last: Option<usize>) {
        let new_last = self.segment_count().checked_sub(1);
        if let Some(index) = old_last {
            self.expand_chunk(index);
        }
        if new_last != old_last
            && let Some(index) = new_last
        {
            self.expand_chunk(index);
        }
    }

    fn ensure_chunk_for(&mut self, index: usize) {
        if index < self.recent_prefix {
            return;
        }
        let chunk = (index - self.recent_prefix) / FORECAST_TRAIL_CHUNK_SEGMENTS;
        if chunk >= self.chunks.len() {
            self.chunks.resize(chunk + 1, ForecastChunkBounds::Empty);
        }
    }

    fn expand_chunk(&mut self, index: usize) {
        if index < self.recent_prefix {
            return;
        }
        self.ensure_chunk_for(index);
        let chunk = (index - self.recent_prefix) / FORECAST_TRAIL_CHUNK_SEGMENTS;
        let bound = match self.segment(index) {
            Some((start, end)) if start.is_finite() && end.is_finite() => {
                ForecastChunkBounds::Finite {
                    min: start.min(end),
                    max: start.max(end),
                }
            }
            _ => ForecastChunkBounds::Invalid,
        };
        self.chunks[chunk] = match (self.chunks[chunk], bound) {
            (ForecastChunkBounds::Invalid, _) | (_, ForecastChunkBounds::Invalid) => {
                ForecastChunkBounds::Invalid
            }
            (ForecastChunkBounds::Empty, bound) => bound,
            (
                ForecastChunkBounds::Finite { min, max },
                ForecastChunkBounds::Finite {
                    min: new_min,
                    max: new_max,
                },
            ) => ForecastChunkBounds::Finite {
                min: min.min(new_min),
                max: max.max(new_max),
            },
            (existing, ForecastChunkBounds::Empty) => existing,
        };
    }

    /// Append only chunks whose conservative bounds can meet the swept
    /// capsule. Missing or invalid cache entries include their indices rather
    /// than risking a false negative.
    fn append_overlapping_segments(
        &self,
        p0: Vec2,
        p1: Vec2,
        radius: f32,
        first_index: usize,
        output: &mut Vec<usize>,
    ) {
        let segment_count = self.segment_count();
        let first_index = first_index.min(segment_count);
        if first_index >= segment_count {
            return;
        }
        if !p0.is_finite() || !p1.is_finite() || !radius.is_finite() || radius < 0.0 {
            output.extend(first_index..segment_count);
            return;
        }
        // The constructor normally makes first_index fall in the cached
        // prefix. Keep the uncached part conservative if this method is used
        // with an older start index.
        let cached_first = first_index.max(self.recent_prefix);
        if first_index < cached_first {
            output.extend(first_index..cached_first.min(segment_count));
        }
        if cached_first >= segment_count {
            return;
        }
        let first_chunk = (cached_first - self.recent_prefix) / FORECAST_TRAIL_CHUNK_SEGMENTS;
        let last_chunk =
            (segment_count - 1).saturating_sub(self.recent_prefix) / FORECAST_TRAIL_CHUNK_SEGMENTS;
        for chunk_index in first_chunk..=last_chunk {
            let chunk_start = self.recent_prefix + chunk_index * FORECAST_TRAIL_CHUNK_SEGMENTS;
            let start = first_index.max(chunk_start);
            let end = segment_count.min(chunk_start + FORECAST_TRAIL_CHUNK_SEGMENTS);
            if start >= end {
                continue;
            }
            let Some(bound) = self.chunks.get(chunk_index).copied() else {
                output.extend(start..end);
                continue;
            };
            let overlaps = match bound {
                ForecastChunkBounds::Finite { min, max } if min.is_finite() && max.is_finite() => {
                    let scale = p0
                        .abs()
                        .max(p1.abs())
                        .max(min.abs())
                        .max(max.abs())
                        .max_element()
                        .max(1.0);
                    let padding = Vec2::splat(radius + scale * f32::EPSILON * 8.0);
                    !(p0.min(p1).cmpgt(max + padding).any()
                        || p0.max(p1).cmplt(min - padding).any())
                }
                ForecastChunkBounds::Empty
                | ForecastChunkBounds::Finite { .. }
                | ForecastChunkBounds::Invalid => true,
            };
            if overlaps {
                output.extend(start..end);
            }
        }
    }
}

impl TrailSegmentAccessor for ForecastTrail<'_> {
    fn segment_count(&self) -> usize {
        self.prefix
            .map_or(0, |prefix| prefix.points.len().saturating_sub(1))
            + self.points.len()
            + usize::from(
                self.points
                    .last()
                    .is_none_or(|point| point.distance_squared(self.head) > 1e-8)
                    && (!self.points.is_empty() || self.head.distance_squared(self.anchor) > 1e-8),
            )
    }

    fn segment(&self, index: usize) -> Option<(Vec2, Vec2)> {
        // The former exact head is not a committed sample. Appending moves
        // that segment's endpoint; it must not freeze it into the prefix.
        let prefix_count = self
            .prefix
            .map_or(0, |prefix| prefix.points.len().saturating_sub(1));
        if index < prefix_count {
            return self.prefix.and_then(|prefix| prefix.segment(index));
        }
        let relative = index - prefix_count;
        if relative < self.points.len() {
            let start = if relative == 0 {
                self.anchor
            } else {
                self.points[relative - 1]
            };
            return Some((start, self.points[relative]));
        }
        (relative == self.points.len())
            .then(|| {
                (
                    self.points.last().copied().unwrap_or(self.anchor),
                    self.head,
                )
            })
            .filter(|(start, end)| start.distance_squared(*end) > 1e-8)
    }
}

/// A forecast either has to reach the route endpoint or only remain safe for
/// its bounded horizon. This replaces the old ambiguous `require_arrival` bit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ForecastGoal {
    ReachEndpoint { waypoint_threshold: f32 },
    SafeHorizon { waypoint_threshold: f32 },
}

impl ForecastGoal {
    fn waypoint_threshold(self) -> f32 {
        match self {
            Self::ReachEndpoint { waypoint_threshold }
            | Self::SafeHorizon { waypoint_threshold } => waypoint_threshold,
        }
    }

    fn accepts_horizon(self) -> bool {
        matches!(self, Self::SafeHorizon { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RouteForecastStatus {
    Rejected,
    Reached,
    SafeHorizon,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RouteForecast {
    pub status: RouteForecastStatus,
    pub elapsed: f32,
    pub clearance: f32,
}

impl RouteForecast {
    pub fn rejected(elapsed: f32) -> Self {
        Self {
            status: RouteForecastStatus::Rejected,
            elapsed,
            clearance: 0.0,
        }
    }

    pub fn reached(elapsed: f32, clearance: f32) -> Self {
        Self {
            status: RouteForecastStatus::Reached,
            elapsed,
            clearance,
        }
    }

    pub fn is_reached(self) -> bool {
        self.status == RouteForecastStatus::Reached
    }

    pub fn is_viable(self) -> bool {
        self.status != RouteForecastStatus::Rejected
    }
}

/// All route forecasts use this fixed-tick kernel. Inputs are held between
/// competence think updates, while movement advances at the authoritative
/// fixed step. The general route wrapper defers clearance scoring until the
/// complete requested motion passes safety, so rejected routes never pay for
/// rival prediction.
pub(crate) fn forecast_route<F>(
    request: ForecastRequest<'_>,
    scratch: &mut CaptureScratch,
    mut clearance_at: F,
) -> RouteForecast
where
    F: FnMut(Vec2, f32) -> f32,
{
    let mut samples = std::mem::take(&mut scratch.forecast_samples);
    samples.clear();
    let mut result = forecast_path(request, scratch, |position, elapsed| {
        samples.push((position, elapsed));
        true
    });
    if result.is_viable() {
        for &(position, elapsed) in &samples {
            result.clearance = result.clearance.min(clearance_at(position, elapsed));
        }
    }
    scratch.forecast_samples = samples;
    result
}

pub(super) fn forecast_path<F>(
    request: ForecastRequest<'_>,
    scratch: &mut CaptureScratch,
    mut on_sample: F,
) -> RouteForecast
where
    F: FnMut(Vec2, f32) -> bool,
{
    let context = request.context;
    let route = request.route;
    let dt = context.config.fixed_delta_seconds();
    if request.max_ticks > FORECAST_ROUTE_STEP_CAP
        || route.count == 0
        || request.route_index > route.count as usize
        || !context.motion.position.is_finite()
        || !context.speed.is_finite()
        || !dt.is_finite()
        || dt <= 0.0
    {
        return RouteForecast::rejected(0.0);
    }

    let mut forecast = context.motion;
    let mut target_index = request.route_index;
    let think_interval = 1.0 / context.competence.think_hz().max(1.0);
    let mut think_remaining = 0.0;
    let mut desired = None;
    let minimum_clearance = f32::INFINITY;
    // Keep committed trail history borrowed. Only points sampled by this
    // bounded horizon are mutable and need storage.
    let mut future_trail = context
        .own_trail
        .map(|trail| ForecastTrail::from_active(trail, request.max_ticks));
    let first_unindexed = context
        .own_trail
        .map_or(0, ActiveTrail::segment_count)
        .saturating_sub(NPC_RELEVANT_TRAIL_SEGMENT_CAP);
    // The board is immutable throughout this forecast. Adjacent fixed steps
    // usually cover the same raster buckets, so retain their exact selection.
    let mut last_query_bounds = None;
    scratch.trail_refs.clear();
    let waypoint_threshold = request.goal.waypoint_threshold();

    for tick in 0..request.max_ticks {
        // Controller route advancement occurs only when it thinks. In
        // particular, do not continuously retarget every fixed movement tick.
        if desired.is_none() || think_remaining <= f32::EPSILON {
            if target_index >= route.count as usize {
                return RouteForecast::reached(tick as f32 * dt, minimum_clearance);
            }
            if route.points[target_index].distance(forecast.position) <= waypoint_threshold {
                target_index += 1;
                if target_index >= route.count as usize {
                    return RouteForecast::reached(tick as f32 * dt, minimum_clearance);
                }
            }
            let target = route.points[target_index];
            if !target.is_finite() {
                return RouteForecast::rejected(tick as f32 * dt);
            }
            desired = Some((target - forecast.position).try_normalize());
            think_remaining = think_interval;
        }

        let next = predict_motion(
            forecast,
            desired.flatten(),
            context.territory,
            context.config,
            context.speed,
            dt,
        );
        if !next.position.is_finite()
            || !context
                .territory
                .arena_boundary()
                .at_least_margin(next.position, context.config.collision_radius)
        {
            return RouteForecast::rejected((tick + 1) as f32 * dt);
        }

        // Deliberately ordered like extend_trails: append the exact head using
        // production sampling rules, then query old and newly grown segments.
        if future_trail.is_none()
            && context.territory.owns(forecast.position, context.id)
            && !context.territory.owns(next.position, context.id)
        {
            let boundary =
                context
                    .territory
                    .boundary_crossing(context.id, forecast.position, next.position);
            future_trail = Some(ForecastTrail::new(
                boundary,
                next.heading,
                request.max_ticks.saturating_sub(tick),
            ));
        }
        if let Some(trail) = future_trail.as_mut() {
            trail.append(
                next.position,
                next.heading,
                context.config.trail_sample_distance,
                context.config.trail_sample_angle_radians,
            );
        }
        if let Some(trail) = future_trail.as_mut() {
            scratch.trail_candidates.clear();
            // The synthetic range below already includes all short trails.
            // Querying hundreds of board cells per forecast tick cannot add
            // another segment in that case; rivals are not self-collisions.
            if first_unindexed > 0 {
                let query_radius = context.config.collision_radius
                    + context.config.trail_width * 0.5
                    + next.position.distance(forecast.position)
                    + TRAIL_QUERY_RADIUS;
                let center = forecast.position.lerp(next.position, 0.5);
                let bounds = context.board.clamped_cell_bounds(
                    center - Vec2::splat(query_radius),
                    center + Vec2::splat(query_radius),
                );
                if bounds != last_query_bounds {
                    collect_indexed_self_segments(
                        context.board,
                        center,
                        query_radius,
                        context.id,
                        first_unindexed,
                        &mut scratch.trail_refs,
                    );
                    last_query_bounds = bounds;
                }
                scratch
                    .trail_candidates
                    .extend(scratch.trail_refs.iter().map(|reference| reference.segment));
            }
            // Raster buckets cannot contain synthetic future segments. The
            // chunk broadphase retains exact candidate semantics while avoiding
            // narrow-phase work for distant recent and synthetic segments.
            trail.append_overlapping_segments(
                forecast.position,
                next.position,
                context.config.collision_radius + context.config.trail_width * 0.5,
                first_unindexed,
                &mut scratch.trail_candidates,
            );
            // Older indices are sorted and strictly below first_unindexed;
            // the concatenation is already sorted and duplicate-free.
            let self_collision = swept_self_active_trail_impact(
                forecast.position,
                next.position,
                context.config.collision_radius + context.config.trail_width * 0.5,
                trail,
                context.config.self_trail_exclusion_distance,
                &scratch.trail_candidates,
            );
            if self_collision.is_some() {
                return RouteForecast::rejected((tick + 1) as f32 * dt);
            }
        }

        forecast = next;
        let elapsed = (tick + 1) as f32 * dt;
        if !on_sample(forecast.position, elapsed) {
            return RouteForecast::rejected(elapsed);
        }
        think_remaining = (think_remaining - dt).max(0.0);
    }

    if request.goal.accepts_horizon() {
        RouteForecast {
            status: RouteForecastStatus::SafeHorizon,
            elapsed: request.max_ticks as f32 * dt,
            clearance: minimum_clearance,
        }
    } else {
        RouteForecast::rejected(request.max_ticks as f32 * dt)
    }
}

pub(crate) struct ForecastRequest<'a> {
    pub context: ForecastContext<'a>,
    pub route: &'a NpcRoute,
    pub route_index: usize,
    pub max_ticks: usize,
    pub goal: ForecastGoal,
}

/// Match full collection → own/older filter → sorted first K, without ever
/// materializing all references in a dense bucket. Recent/synthetic indices
/// are appended separately, so this selection cannot consume their budget.
fn collect_indexed_self_segments(
    board: &BoardGrid,
    center: Vec2,
    radius: f32,
    owner: CompetitorId,
    before_segment: usize,
    output: &mut Vec<crate::board::TrailSegmentRef>,
) {
    output.clear();
    // New and short trails have no indexed history outside the exact recent
    // tail. Avoid visiting every nearby bucket only to reject every reference.
    if before_segment == 0 {
        return;
    }
    board.visit_nearby_trail_segments(center, radius, |reference| {
        if reference.owner != owner || reference.segment >= before_segment {
            return;
        }
        if output.len() == NPC_RELEVANT_TRAIL_SEGMENT_CAP
            && output
                .last()
                .is_some_and(|last| reference.segment >= last.segment)
        {
            return;
        }
        if let Err(index) = output.binary_search_by_key(&reference.segment, |value| value.segment)
            && index < NPC_RELEVANT_TRAIL_SEGMENT_CAP
        {
            output.insert(index, reference);
            output.truncate(NPC_RELEVANT_TRAIL_SEGMENT_CAP);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_selection_matches_full_collection_at_history_boundaries() {
        let mut board = BoardGrid::generate(42, 2, &GameConfig::default());
        let cell = board.world_to_cell(Vec2::ZERO).unwrap();
        let index = board.index(cell).unwrap();
        for step in 0..2048 {
            board.trail_segment_buckets[index].push(crate::board::TrailSegmentRef {
                owner: CompetitorId((step % 3) as u8),
                segment: (2048 - step) % 257,
            });
        }
        for before in [0, 1, 95, 96, 97, 300] {
            let mut expected = std::collections::BTreeSet::new();
            board.visit_nearby_trail_segments(Vec2::ZERO, 5.0, |reference| {
                if reference.owner == CompetitorId(1) && reference.segment < before {
                    expected.insert(reference.segment);
                }
            });
            let mut actual = vec![crate::board::TrailSegmentRef {
                owner: CompetitorId(1),
                segment: usize::MAX,
            }];
            collect_indexed_self_segments(
                &board,
                Vec2::ZERO,
                5.0,
                CompetitorId(1),
                before,
                &mut actual,
            );
            assert_eq!(
                actual
                    .iter()
                    .map(|reference| reference.segment)
                    .collect::<Vec<_>>(),
                expected
                    .into_iter()
                    .take(NPC_RELEVANT_TRAIL_SEGMENT_CAP)
                    .collect::<Vec<_>>()
            );
        }
    }
    use crate::{
        board::{BoardGrid, Cell},
        config::GameConfig,
        territory_map::TerritoryMap,
    };

    fn setup() -> (BoardGrid, TerritoryMap) {
        let board = BoardGrid::generate(91, 2, &GameConfig::default());
        let mut territory = TerritoryMap::from_board(&board);
        territory.seed_owner(Vec2::ZERO, 10.0, CompetitorId(0));
        (board, territory)
    }

    fn request<'a>(
        board: &'a BoardGrid,
        territory: &'a TerritoryMap,
        config: &'a GameConfig,
        route: &'a NpcRoute,
        speed: f32,
        goal: ForecastGoal,
    ) -> ForecastRequest<'a> {
        ForecastRequest {
            context: ForecastContext {
                board,
                territory,
                config,
                id: CompetitorId(0),
                motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
                speed,
                own_trail: None,
                competence: NpcCompetence { skill: 0.8 },
            },
            route,
            route_index: 0,
            max_ticks: FORECAST_ROUTE_STEP_CAP,
            goal,
        }
    }

    #[test]
    fn rejected_forecasts_skip_scoring_and_keep_storage_bounded() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let route = NpcRoute::from_points(&[Vec2::X * 10.0], RouteTarget::OpenSpace);
        let mut scratch = CaptureScratch::default();
        for max_ticks in [1, FORECAST_ROUTE_STEP_CAP + 1] {
            let mut request = request(
                &board,
                &territory,
                &config,
                &route,
                config.player_speed,
                ForecastGoal::ReachEndpoint {
                    waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
                },
            );
            request.max_ticks = max_ticks;
            let result = forecast_route(request, &mut scratch, |_, _| {
                panic!("rejected route was scored")
            });
            assert!(!result.is_viable());
            assert!(scratch.forecast_samples.len() <= FORECAST_ROUTE_STEP_CAP);
        }
    }

    #[test]
    fn forecast_holds_steering_between_think_updates() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let route = NpcRoute::from_points(&[Vec2::Y * 10.0], RouteTarget::OpenSpace);
        let mut positions = Vec::new();
        let result = forecast_route(
            ForecastRequest {
                context: ForecastContext {
                    board: &board,
                    territory: &territory,
                    config: &config,
                    id: CompetitorId(0),
                    motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
                    speed: config.player_speed,
                    own_trail: None,
                    competence: NpcCompetence { skill: 0.0 },
                },
                route: &route,
                route_index: 0,
                max_ticks: 2,
                goal: ForecastGoal::SafeHorizon {
                    waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
                },
            },
            &mut CaptureScratch::default(),
            |position, _| {
                positions.push(position);
                f32::INFINITY
            },
        );
        assert!(result.is_viable());
        let mut expected = CompetitorMotion::new(Vec2::ZERO, Vec2::X);
        for _ in 0..2 {
            expected = predict_motion(
                expected,
                Some(Vec2::Y),
                &territory,
                &config,
                config.player_speed,
                config.fixed_delta_seconds(),
            );
        }
        assert_eq!(positions.len(), 2);
        assert!(positions[1].distance(expected.position) < 1e-5);
    }

    #[test]
    fn forecast_uses_runtime_waypoint_arrival_threshold() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let near = NpcRoute::from_points(&[Vec2::new(1.1, 0.0)], RouteTarget::OpenSpace);
        let far = NpcRoute::from_points(&[Vec2::new(1.2, 0.0)], RouteTarget::OpenSpace);
        let goal = ForecastGoal::ReachEndpoint {
            waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
        };
        assert!(
            forecast_route(
                request(
                    &board,
                    &territory,
                    &config,
                    &near,
                    config.player_speed,
                    goal
                ),
                &mut CaptureScratch::default(),
                |_, _| f32::INFINITY,
            )
            .is_reached()
        );
        assert!(
            !forecast_route(
                request(&board, &territory, &config, &far, 0.0, goal),
                &mut CaptureScratch::default(),
                |_, _| f32::INFINITY,
            )
            .is_reached()
        );
    }

    #[test]
    fn forecast_includes_future_sampled_trail_growth() {
        let config = GameConfig::default();
        let board = BoardGrid::generate(91, 2, &config);
        let territory = TerritoryMap::from_board(&board);
        let trail = ActiveTrail::new(CompetitorId(0), Cell::new(0, 0), Vec2::ZERO, Vec2::X);
        let route = NpcRoute::from_points(
            &[
                Vec2::new(4.0, 0.0),
                Vec2::new(4.0, 4.0),
                Vec2::new(0.0, 4.0),
                Vec2::new(2.0, -3.0),
            ],
            RouteTarget::OpenSpace,
        );
        let result = forecast_route(
            ForecastRequest {
                context: ForecastContext {
                    board: &board,
                    territory: &territory,
                    config: &config,
                    id: CompetitorId(0),
                    motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
                    speed: config.player_speed,
                    own_trail: Some(&trail),
                    competence: NpcCompetence { skill: 0.8 },
                },
                route: &route,
                route_index: 0,
                max_ticks: FORECAST_ROUTE_STEP_CAP,
                goal: ForecastGoal::ReachEndpoint {
                    waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
                },
            },
            &mut CaptureScratch::default(),
            |_, _| f32::INFINITY,
        );
        assert_eq!(result.status, RouteForecastStatus::Rejected);
        assert_eq!(trail.segment_count(), 0);
    }

    #[test]
    fn forecast_rejects_a_late_self_trail_intersection() {
        let config = GameConfig::default();
        let board = BoardGrid::generate(91, 2, &config);
        let territory = TerritoryMap::from_board(&board);
        let mut trail = ActiveTrail::new(
            CompetitorId(0),
            Cell::new(0, 0),
            Vec2::new(-10.0, -5.0),
            Vec2::X,
        );
        trail.append_exact(Vec2::new(0.0, -5.0));
        trail.append_exact(Vec2::ZERO);
        let route = NpcRoute::from_points(
            &[
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 10.0),
                Vec2::new(-5.0, 10.0),
                Vec2::new(-5.0, -10.0),
            ],
            RouteTarget::OpenSpace,
        );
        let result = forecast_route(
            ForecastRequest {
                context: ForecastContext {
                    board: &board,
                    territory: &territory,
                    config: &config,
                    id: CompetitorId(0),
                    motion: CompetitorMotion::new(Vec2::ZERO, Vec2::X),
                    speed: config.player_speed,
                    own_trail: Some(&trail),
                    competence: NpcCompetence { skill: 1.0 },
                },
                route: &route,
                route_index: 0,
                max_ticks: FORECAST_ROUTE_STEP_CAP,
                goal: ForecastGoal::ReachEndpoint {
                    waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
                },
            },
            &mut CaptureScratch::default(),
            |_, _| f32::INFINITY,
        );
        assert_eq!(result.status, RouteForecastStatus::Rejected);
    }

    #[test]
    fn bounded_owner_selection_matches_full_collection_in_dense_buckets() {
        let config = GameConfig::default();
        let mut board = BoardGrid::generate(91, 2, &config);
        let bucket = board
            .index(board.world_to_cell(Vec2::ZERO).unwrap())
            .unwrap();
        for _ in 0..2 {
            for owner in 0..crate::ids::MAX_COMPETITORS {
                for segment in (0..300).rev() {
                    board.trail_segment_buckets[bucket].push(crate::board::TrailSegmentRef {
                        owner: CompetitorId(owner as u8),
                        segment,
                    });
                }
            }
        }
        let mut selected = Vec::new();
        for center in [Vec2::ZERO, Vec2::splat(1000.0)] {
            for before in [0, 19, 300] {
                let mut expected = Vec::new();
                board.collect_nearby_trail_segments(center, 3.0, &mut expected);
                expected.retain(|reference| {
                    reference.owner == CompetitorId(1) && reference.segment < before
                });
                expected.truncate(NPC_RELEVANT_TRAIL_SEGMENT_CAP);
                collect_indexed_self_segments(
                    &board,
                    center,
                    3.0,
                    CompetitorId(1),
                    before,
                    &mut selected,
                );
                assert_eq!(selected, expected);
                assert!(selected.len() <= NPC_RELEVANT_TRAIL_SEGMENT_CAP);
                assert!(selected.capacity() <= NPC_RELEVANT_TRAIL_SEGMENT_CAP * 2);
            }
        }
    }

    #[test]
    fn cached_bucket_queries_follow_motion_and_reset_between_forecasts() {
        let config = GameConfig::default();
        let mut board = BoardGrid::generate(91, 2, &config);
        let territory = TerritoryMap::from_board(&board);
        let mut trail = ActiveTrail::new(
            CompetitorId(0),
            Cell::new(0, 0),
            Vec2::new(2.0, -5.0),
            Vec2::Y,
        );
        trail.append_exact(Vec2::new(2.0, 5.0));
        for i in 0..80 {
            trail.append_exact(Vec2::new(-10.0 - i as f32 * 0.02, 10.0));
        }
        trail.append_exact(Vec2::new(-5.0, 0.0));
        assert!(trail.segment_count() > NPC_RELEVANT_TRAIL_SEGMENT_CAP);
        let route = NpcRoute::from_points(&[Vec2::new(5.0, 0.0)], RouteTarget::OpenSpace);
        let mut scratch = CaptureScratch::default();
        let mut run = |board: &BoardGrid| {
            let mut request = request(
                board,
                &territory,
                &config,
                &route,
                config.player_speed,
                ForecastGoal::ReachEndpoint {
                    waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
                },
            );
            request.context.motion = CompetitorMotion::new(Vec2::new(-5.0, 0.0), Vec2::X);
            request.context.own_trail = Some(&trail);
            forecast_route(request, &mut scratch, |_, _| f32::INFINITY)
        };
        assert!(
            run(&board).is_reached(),
            "the old crossing is outside the synthetic tail"
        );
        let bucket = board
            .index(board.world_to_cell(Vec2::new(2.0, 0.0)).unwrap())
            .unwrap();
        board.trail_segment_buckets[bucket].push(crate::board::TrailSegmentRef {
            owner: CompetitorId(0),
            segment: 0,
        });
        assert!(
            !run(&board).is_viable(),
            "moving into a new bucket range must discover old ink"
        );
        board.trail_segment_buckets[bucket].clear();
        assert!(
            run(&board).is_reached(),
            "cached refs must not survive a planning call"
        );
    }

    #[test]
    fn borrowed_forecast_tail_matches_full_clone_across_append_boundaries() {
        let config = GameConfig::default();
        let mut base = ActiveTrail::new(
            CompetitorId(0),
            crate::board::Cell::new(0, 0),
            Vec2::new(-8.0, -4.0),
            Vec2::X,
        );
        // A long history exercises the borrowed prefix and preserves segment
        // identities on both sides of the indexed/recent boundary.
        for index in 0..100 {
            let angle = index as f32 * 0.21;
            base.append_exact(Vec2::new(angle.cos() * 4.0, angle.sin() * 4.0));
        }
        let mut expected = base.clone();
        let mut forecast = ForecastTrail::from_active(&base, 7);
        let operations = [
            (Vec2::new(1.0, 1.0), Vec2::X),
            (Vec2::new(1.05, 1.0), Vec2::Y),
            (Vec2::new(1.15, 1.0), Vec2::Y),
            (Vec2::new(1.30, 1.0), Vec2::Y),
            (Vec2::new(1.30, 1.0), Vec2::X),
            (Vec2::new(1.50, 1.0), Vec2::X),
            (Vec2::new(1.70, 1.0), Vec2::X),
        ];
        for (point, heading) in operations {
            assert_eq!(
                expected.append(
                    point,
                    heading,
                    config.trail_sample_distance,
                    config.trail_sample_angle_radians,
                ),
                forecast.append(
                    point,
                    heading,
                    config.trail_sample_distance,
                    config.trail_sample_angle_radians,
                )
            );
            assert_eq!(expected.head, forecast.head);
            assert_eq!(expected.length, forecast.length);
            assert_eq!(expected.segment_count(), forecast.segment_count());
            for index in 0..expected.segment_count() {
                assert_eq!(expected.segment(index), forecast.segment(index));
            }
            assert!(forecast.points.len() <= 7);
        }
    }

    #[test]
    fn borrowed_forecast_moves_uncommitted_head_instead_of_freezing_it() {
        let mut base = ActiveTrail::new(CompetitorId(0), Cell::new(0, 0), Vec2::ZERO, Vec2::X);
        base.append(Vec2::new(0.1, 0.0), Vec2::X, 0.5, 0.2);
        assert_ne!(base.points.last().copied(), Some(base.head));
        let mut expected = base.clone();
        let mut forecast = ForecastTrail::from_active(&base, 3);
        for point in [
            Vec2::new(0.2, 0.05),
            Vec2::new(0.7, 0.0),
            Vec2::new(0.8, 0.03),
        ] {
            assert_eq!(
                expected.append(point, Vec2::X, 0.5, 0.2),
                forecast.append(point, Vec2::X, 0.5, 0.2)
            );
            assert_eq!(expected.segment_count(), forecast.segment_count());
            let full_candidates: Vec<_> = (0..expected.segment_count()).collect();
            let mut chunk_candidates = Vec::new();
            forecast.append_overlapping_segments(
                Vec2::new(-1.0, 0.0),
                Vec2::new(1.0, 0.0),
                0.2,
                0,
                &mut chunk_candidates,
            );
            for excluded in [0.0, 0.01, 0.5, 1.0] {
                assert_eq!(
                    swept_self_active_trail_impact(
                        Vec2::new(-1.0, 0.0),
                        Vec2::new(1.0, 0.0),
                        0.2,
                        &expected,
                        excluded,
                        &full_candidates,
                    ),
                    swept_self_active_trail_impact(
                        Vec2::new(-1.0, 0.0),
                        Vec2::new(1.0, 0.0),
                        0.2,
                        &forecast,
                        excluded,
                        &chunk_candidates,
                    ),
                    "excluded distance {excluded}"
                );
            }
            for index in 0..expected.segment_count() + 1 {
                assert_eq!(expected.segment(index), forecast.segment(index));
            }
        }
    }

    #[test]
    fn borrowed_forecast_tail_matches_clone_for_self_intersections_and_cutoffs() {
        let mut base = ActiveTrail::new(
            CompetitorId(0),
            crate::board::Cell::new(0, 0),
            Vec2::new(-4.0, -4.0),
            Vec2::X,
        );
        for point in [
            Vec2::new(4.0, -4.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(-4.0, 4.0),
            Vec2::new(-4.0, -4.0),
            Vec2::new(0.0, 0.0),
        ] {
            base.append_exact(point);
        }
        let mut expected = base.clone();
        let mut forecast = ForecastTrail::from_active(&base, 4);
        let first_relevant = base
            .segment_count()
            .saturating_sub(NPC_RELEVANT_TRAIL_SEGMENT_CAP);
        for (point, heading) in [
            (Vec2::new(4.0, 0.0), Vec2::X),
            (Vec2::new(0.0, 0.0), -Vec2::X),
            (Vec2::new(-4.0, 0.0), -Vec2::X),
            (Vec2::new(-4.0, -3.0), Vec2::Y),
        ] {
            expected.append(point, heading, 0.6, 0.4);
            forecast.append(point, heading, 0.6, 0.4);
            let full_candidates: Vec<_> = (first_relevant..expected.segment_count()).collect();
            let mut chunk_candidates = Vec::new();
            forecast.append_overlapping_segments(
                Vec2::new(-5.0, 0.0),
                Vec2::new(5.0, 0.0),
                0.2,
                first_relevant,
                &mut chunk_candidates,
            );
            for excluded in [0.0, 0.01, 1.0, 7.999, 8.0, 20.0] {
                assert_eq!(
                    swept_self_active_trail_impact(
                        Vec2::new(-5.0, 0.0),
                        Vec2::new(5.0, 0.0),
                        0.2,
                        &expected,
                        excluded,
                        &full_candidates,
                    ),
                    swept_self_active_trail_impact(
                        Vec2::new(-5.0, 0.0),
                        Vec2::new(5.0, 0.0),
                        0.2,
                        &forecast,
                        excluded,
                        &chunk_candidates,
                    ),
                    "excluded distance {excluded}"
                );
            }
        }
    }

    #[test]
    fn chunk_broadphase_reduces_a_straight_recent_tail_without_false_negatives() {
        let mut base = ActiveTrail::new(CompetitorId(0), Cell::new(0, 0), Vec2::ZERO, Vec2::X);
        for index in 1..=200 {
            base.append_exact(Vec2::new(index as f32, 0.0));
        }
        let forecast = ForecastTrail::from_active(&base, FORECAST_ROUTE_STEP_CAP);
        let first_relevant = base
            .segment_count()
            .saturating_sub(NPC_RELEVANT_TRAIL_SEGMENT_CAP);
        let mut candidates = Vec::new();
        forecast.append_overlapping_segments(
            Vec2::new(198.0, 0.0),
            Vec2::new(199.0, 0.0),
            0.2,
            first_relevant,
            &mut candidates,
        );
        assert!(!candidates.is_empty());
        assert!(candidates.len() < base.segment_count() - first_relevant);
        assert!(candidates.iter().all(|&index| index >= first_relevant));
    }

    #[test]
    fn new_forecast_tail_preserves_active_trail_head_transitions() {
        let boundary = Vec2::new(2.0, 0.0);
        let mut expected = ActiveTrail::new(
            CompetitorId(0),
            crate::board::Cell::new(0, 0),
            boundary,
            Vec2::X,
        );
        let mut forecast = ForecastTrail::new(boundary, Vec2::X, 3);
        for (point, heading) in [
            (Vec2::new(2.1, 0.0), Vec2::X),
            (Vec2::new(2.2, 0.0), Vec2::X),
            (Vec2::new(2.2, 0.0), Vec2::Y),
        ] {
            assert_eq!(
                expected.append(point, heading, 0.5, 0.2),
                forecast.append(point, heading, 0.5, 0.2)
            );
            assert_eq!(expected.segment_count(), forecast.segment_count());
            for index in 0..expected.segment_count() {
                assert_eq!(expected.segment(index), forecast.segment(index));
            }
        }
        assert!(forecast.points.capacity() <= 3);
    }

    #[test]
    fn forecast_path_stops_after_a_safe_sample_when_callback_rejects() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let route = NpcRoute::from_points(&[Vec2::new(4.0, 0.0)], RouteTarget::OpenSpace);
        let mut calls = 0;
        let result = forecast_path(
            request(
                &board,
                &territory,
                &config,
                &route,
                config.player_speed,
                ForecastGoal::ReachEndpoint {
                    waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
                },
            ),
            &mut CaptureScratch::default(),
            |_, _| {
                calls += 1;
                calls < 2
            },
        );
        assert_eq!(result.status, RouteForecastStatus::Rejected);
        assert_eq!(calls, 2);
    }

    #[test]
    fn forecast_distinguishes_rejected_reached_and_safe_horizon() {
        let (board, territory) = setup();
        let config = GameConfig::default();
        let route = NpcRoute::from_points(&[Vec2::new(4.0, 0.0)], RouteTarget::OpenSpace);
        let rejected = forecast_route(
            request(
                &board,
                &territory,
                &config,
                &route,
                0.0,
                ForecastGoal::ReachEndpoint {
                    waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
                },
            ),
            &mut CaptureScratch::default(),
            |_, _| f32::INFINITY,
        );
        assert_eq!(rejected.status, RouteForecastStatus::Rejected);

        let reached_route = NpcRoute::from_points(&[Vec2::new(1.1, 0.0)], RouteTarget::OpenSpace);
        let reached = forecast_route(
            request(
                &board,
                &territory,
                &config,
                &reached_route,
                config.player_speed,
                ForecastGoal::ReachEndpoint {
                    waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
                },
            ),
            &mut CaptureScratch::default(),
            |_, _| f32::INFINITY,
        );
        assert_eq!(reached.status, RouteForecastStatus::Reached);

        let safe = forecast_route(
            ForecastRequest {
                max_ticks: 0,
                ..request(
                    &board,
                    &territory,
                    &config,
                    &route,
                    config.player_speed,
                    ForecastGoal::SafeHorizon {
                        waypoint_threshold: WAYPOINT_ARRIVAL_DISTANCE,
                    },
                )
            },
            &mut CaptureScratch::default(),
            |_, _| f32::INFINITY,
        );
        assert_eq!(safe.status, RouteForecastStatus::SafeHorizon);
    }
}
