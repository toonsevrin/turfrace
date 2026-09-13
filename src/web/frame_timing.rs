//! Opt-in wall-clock instrumentation for the browser main loop.
//!
//! This measures time spent by the Bevy main app schedules, not GPU work or
//! presentation. The accumulator is deliberately bounded to scalar counters
//! and paired maxima so it is also useful in native unit tests.

use bevy::prelude::*;

const SIXTY_FPS_MS: f64 = 1000.0 / 60.0;
const THIRTY_FPS_MS: f64 = 1000.0 / 30.0;
const PUBLISH_INTERVAL_MS: f64 = 200.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameTimingSample {
    /// Wall duration from the First hook through the Last hook.
    pub total_main_app_ms: f64,
    /// Sum of FixedFirst-to-FixedLast wall durations for this rendered frame.
    pub fixed_ms: f64,
    /// Number of fixed schedule executions observed between the frame hooks.
    pub fixed_tick_count: u32,
    /// Longest individual fixed schedule execution in this frame.
    pub max_fixed_tick_ms: f64,
    /// `max(total_main_app_ms - fixed_ms, 0)`. This is other measured main-app
    /// work, not GPU/render/presentation time.
    pub remainder_ms: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameTimingSnapshot {
    pub generation: u64,
    pub rendered_frames: u64,
    /// Cumulative wall time measured by the main-app hooks since match start.
    pub total_main_app_ms_sum: f64,
    /// Cumulative fixed-schedule wall time since match start.
    pub fixed_ms_sum: f64,
    pub fixed_tick_count: u64,
    /// Cumulative non-fixed remainder since match start.
    pub remainder_ms_sum: f64,
    pub over16_67ms: u64,
    pub over33_33ms: u64,
    pub max_fixed_ticks_per_frame: u32,
    pub slowest_frame: FrameTimingSample,
    pub slowest_frame_number: u64,
}

/// Cumulative match-local frame measurements. No per-frame samples are kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameTimingAccumulator {
    enabled: bool,
    generation: Option<u64>,
    rendered_frames: u64,
    total_main_app_ms_sum: f64,
    fixed_ms_sum: f64,
    fixed_tick_count: u64,
    remainder_ms_sum: f64,
    over16_67ms: u64,
    over33_33ms: u64,
    max_fixed_ticks_per_frame: u32,
    slowest_frame: Option<FrameTimingSample>,
    slowest_frame_number: u64,
}

impl Default for FrameTimingAccumulator {
    fn default() -> Self {
        Self::new(false)
    }
}

impl FrameTimingAccumulator {
    pub const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            generation: None,
            rendered_frames: 0,
            total_main_app_ms_sum: 0.0,
            fixed_ms_sum: 0.0,
            fixed_tick_count: 0,
            remainder_ms_sum: 0.0,
            over16_67ms: 0,
            over33_33ms: 0,
            max_fixed_ticks_per_frame: 0,
            slowest_frame: None,
            slowest_frame_number: 0,
        }
    }

    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.clear_measurements();
    }

    /// Reset all cumulative values at the start of a playable match.
    pub fn reset_for_match(&mut self, generation: u64) {
        let enabled = self.enabled;
        *self = Self::new(enabled);
        self.generation = Some(generation);
    }

    pub fn generation(&self) -> Option<u64> {
        self.generation
    }

    pub fn record_frame(&mut self, sample: FrameTimingSample) -> bool {
        if !self.enabled
            || !valid_duration(sample.total_main_app_ms)
            || !valid_duration(sample.fixed_ms)
            || !valid_duration(sample.max_fixed_tick_ms)
            || !valid_duration(sample.remainder_ms)
            || (sample.fixed_tick_count == 0 && sample.max_fixed_tick_ms != 0.0)
        {
            return false;
        }
        let remainder_ms = (sample.total_main_app_ms - sample.fixed_ms).max(0.0);
        let sample = FrameTimingSample {
            remainder_ms,
            ..sample
        };
        self.rendered_frames = self.rendered_frames.saturating_add(1);
        add_finite(&mut self.total_main_app_ms_sum, sample.total_main_app_ms);
        add_finite(&mut self.fixed_ms_sum, sample.fixed_ms);
        add_finite(&mut self.remainder_ms_sum, sample.remainder_ms);
        self.fixed_tick_count = self
            .fixed_tick_count
            .saturating_add(u64::from(sample.fixed_tick_count));
        if sample.total_main_app_ms > SIXTY_FPS_MS {
            self.over16_67ms = self.over16_67ms.saturating_add(1);
        }
        if sample.total_main_app_ms > THIRTY_FPS_MS {
            self.over33_33ms = self.over33_33ms.saturating_add(1);
        }
        self.max_fixed_ticks_per_frame =
            self.max_fixed_ticks_per_frame.max(sample.fixed_tick_count);
        // Ties intentionally retain the first frame. Every companion value is
        // copied from this same sample, avoiding unrelated per-field maxima.
        if self
            .slowest_frame
            .is_none_or(|slowest| sample.total_main_app_ms > slowest.total_main_app_ms)
        {
            self.slowest_frame = Some(sample);
            self.slowest_frame_number = self.rendered_frames;
        }
        true
    }

    pub fn snapshot(&self) -> Option<FrameTimingSnapshot> {
        Some(FrameTimingSnapshot {
            generation: self.generation?,
            rendered_frames: self.rendered_frames,
            total_main_app_ms_sum: self.total_main_app_ms_sum,
            fixed_ms_sum: self.fixed_ms_sum,
            fixed_tick_count: self.fixed_tick_count,
            remainder_ms_sum: self.remainder_ms_sum,
            over16_67ms: self.over16_67ms,
            over33_33ms: self.over33_33ms,
            max_fixed_ticks_per_frame: self.max_fixed_ticks_per_frame,
            slowest_frame: self.slowest_frame?,
            slowest_frame_number: self.slowest_frame_number,
        })
    }

    fn clear_measurements(&mut self) {
        *self = Self::new(self.enabled);
    }
}

fn valid_duration(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn add_finite(sum: &mut f64, value: f64) {
    *sum = if *sum >= f64::MAX - value {
        f64::MAX
    } else {
        *sum + value
    };
}

/// Runtime state around the scalar accumulator. It is inserted only when the
/// `?performance` bridge is enabled, so ordinary play has no timing hooks.
#[derive(Resource, Debug)]
pub struct FrameTiming {
    pub accumulator: FrameTimingAccumulator,
    frame_start_ms: Option<f64>,
    fixed_tick_start_ms: Option<f64>,
    fixed_ms_this_frame: f64,
    fixed_tick_count_this_frame: u32,
    max_fixed_tick_ms_this_frame: f64,
    last_published_at_ms: Option<f64>,
    published: Option<FrameTimingSnapshot>,
    publication_pending: bool,
    reset_notice: bool,
}

impl FrameTiming {
    pub fn enabled() -> Self {
        Self {
            accumulator: FrameTimingAccumulator::new(true),
            frame_start_ms: None,
            fixed_tick_start_ms: None,
            fixed_ms_this_frame: 0.0,
            fixed_tick_count_this_frame: 0,
            max_fixed_tick_ms_this_frame: 0.0,
            last_published_at_ms: None,
            published: None,
            publication_pending: false,
            reset_notice: false,
        }
    }

    pub fn published(&self) -> Option<FrameTimingSnapshot> {
        self.published
    }

    pub fn take_publication(&mut self) -> Option<FrameTimingSnapshot> {
        if !self.publication_pending {
            return None;
        }
        self.publication_pending = false;
        self.published
    }

    pub fn take_reset_notice(&mut self) -> bool {
        std::mem::replace(&mut self.reset_notice, false)
    }

    fn begin_frame(&mut self, generation: u64, now_ms: f64) {
        if self.accumulator.generation() != Some(generation) {
            self.accumulator.reset_for_match(generation);
            self.published = None;
            self.publication_pending = false;
            self.last_published_at_ms = None;
            self.reset_notice = true;
        }
        self.clear_current_frame();
        self.frame_start_ms = valid_clock(now_ms).then_some(now_ms);
    }

    fn clear_current_frame(&mut self) {
        self.frame_start_ms = None;
        self.fixed_tick_start_ms = None;
        self.fixed_ms_this_frame = 0.0;
        self.fixed_tick_count_this_frame = 0;
        self.max_fixed_tick_ms_this_frame = 0.0;
    }

    fn begin_fixed_tick(&mut self, now_ms: f64) {
        if self.frame_start_ms.is_some() {
            self.fixed_tick_start_ms = valid_clock(now_ms).then_some(now_ms);
        }
    }

    fn end_fixed_tick(&mut self, now_ms: f64) {
        let Some(start) = self.fixed_tick_start_ms.take() else {
            return;
        };
        let Some(duration) = valid_clock(now_ms)
            .then_some(now_ms - start)
            .filter(|duration| valid_duration(*duration))
        else {
            return;
        };
        add_finite(&mut self.fixed_ms_this_frame, duration);
        self.fixed_tick_count_this_frame = self.fixed_tick_count_this_frame.saturating_add(1);
        self.max_fixed_tick_ms_this_frame = self.max_fixed_tick_ms_this_frame.max(duration);
    }

    fn end_frame(&mut self, now_ms: f64, force_publish: bool) {
        let Some(start) = self.frame_start_ms.take() else {
            return;
        };
        let Some(total_ms) = valid_clock(now_ms)
            .then_some(now_ms - start)
            .filter(|duration| valid_duration(*duration))
        else {
            self.clear_current_frame();
            return;
        };
        self.accumulator.record_frame(FrameTimingSample {
            total_main_app_ms: total_ms,
            fixed_ms: self.fixed_ms_this_frame,
            fixed_tick_count: self.fixed_tick_count_this_frame,
            max_fixed_tick_ms: self.max_fixed_tick_ms_this_frame,
            remainder_ms: 0.0,
        });
        self.fixed_tick_start_ms = None;
        if let Some(snapshot) = self.accumulator.snapshot() {
            let due = self
                .last_published_at_ms
                .is_none_or(|last| now_ms - last >= PUBLISH_INTERVAL_MS);
            if due || force_publish {
                self.published = Some(snapshot);
                self.publication_pending = true;
                self.last_published_at_ms = Some(now_ms);
            }
        }
        self.clear_current_frame();
    }
}

fn valid_clock(value: f64) -> bool {
    value.is_finite()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function turfrace_performance_now() {
    return globalThis.performance?.now?.() ?? Date.now();
}
"#)]
extern "C" {
    fn turfrace_performance_now() -> f64;
}

#[cfg(target_arch = "wasm32")]
fn now_ms() -> f64 {
    turfrace_performance_now()
}

#[cfg(target_arch = "wasm32")]
pub fn begin_frame(
    mut timing: ResMut<FrameTiming>,
    session: Option<Res<crate::match_game::MatchSession>>,
    generation: Option<Res<crate::match_game::MatchGeneration>>,
) {
    let Some(session) = session else {
        timing.clear_current_frame();
        return;
    };
    if session.purpose != crate::match_game::MatchPurpose::Playable
        || session.phase != crate::match_game::MatchPhase::Running
    {
        timing.clear_current_frame();
        return;
    }
    let Some(generation) = generation else {
        timing.clear_current_frame();
        return;
    };
    timing.begin_frame(generation.0, now_ms());
}

#[cfg(target_arch = "wasm32")]
pub fn begin_fixed_tick(mut timing: ResMut<FrameTiming>) {
    timing.begin_fixed_tick(now_ms());
}

#[cfg(target_arch = "wasm32")]
pub fn end_fixed_tick(mut timing: ResMut<FrameTiming>) {
    timing.end_fixed_tick(now_ms());
}

#[cfg(target_arch = "wasm32")]
pub fn end_frame(
    mut timing: ResMut<FrameTiming>,
    session: Option<Res<crate::match_game::MatchSession>>,
) {
    let force_publish = session.is_none_or(|session| {
        session.purpose != crate::match_game::MatchPurpose::Playable
            || session.phase != crate::match_game::MatchPhase::Running
    });
    timing.end_frame(now_ms(), force_publish);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(total: f64, fixed: f64, ticks: u32, max_tick: f64) -> FrameTimingSample {
        FrameTimingSample {
            total_main_app_ms: total,
            fixed_ms: fixed,
            fixed_tick_count: ticks,
            max_fixed_tick_ms: max_tick,
            remainder_ms: 0.0,
        }
    }

    #[test]
    fn disabled_accumulator_does_not_record_or_reset_enabled_state() {
        let mut timing = FrameTimingAccumulator::default();
        assert!(!timing.record_frame(sample(50.0, 10.0, 1, 10.0)));
        assert_eq!(timing.snapshot(), None);
        timing.set_enabled(true);
        timing.reset_for_match(4);
        assert!(timing.record_frame(sample(20.0, 10.0, 1, 10.0)));
        timing.set_enabled(false);
        assert_eq!(timing.snapshot(), None);
        assert!(!timing.is_enabled());
    }

    #[test]
    fn aggregation_counts_thresholds_steps_and_finite_sums() {
        let mut timing = FrameTimingAccumulator::new(true);
        timing.reset_for_match(9);
        assert!(timing.record_frame(sample(16.0, 8.0, 1, 8.0)));
        assert!(timing.record_frame(sample(34.0, 20.0, 2, 11.0)));
        let result = timing.snapshot().unwrap();
        assert_eq!(result.rendered_frames, 2);
        assert_eq!(result.fixed_tick_count, 3);
        assert_eq!(result.over16_67ms, 1);
        assert_eq!(result.over33_33ms, 1);
        assert_eq!(result.max_fixed_ticks_per_frame, 2);
        assert_eq!(result.total_main_app_ms_sum, 50.0);
        assert_eq!(result.fixed_ms_sum, 28.0);
        assert_eq!(result.remainder_ms_sum, 22.0);
    }

    #[test]
    fn slowest_frame_keeps_paired_values_and_first_tie() {
        let mut timing = FrameTimingAccumulator::new(true);
        timing.reset_for_match(2);
        timing.record_frame(sample(40.0, 7.0, 1, 7.0));
        timing.record_frame(sample(40.0, 19.0, 3, 9.0));
        assert_eq!(timing.snapshot().unwrap().slowest_frame_number, 1);
        let slowest = timing.snapshot().unwrap().slowest_frame;
        assert_eq!(slowest.total_main_app_ms, 40.0);
        assert_eq!(slowest.fixed_ms, 7.0);
        assert_eq!(slowest.fixed_tick_count, 1);
        assert_eq!(slowest.max_fixed_tick_ms, 7.0);
        assert_eq!(slowest.remainder_ms, 33.0);
    }

    #[test]
    fn malformed_samples_are_ignored() {
        let mut timing = FrameTimingAccumulator::new(true);
        timing.reset_for_match(1);
        assert!(!timing.record_frame(sample(f64::NAN, 0.0, 0, 0.0)));
        assert!(!timing.record_frame(sample(1.0, f64::INFINITY, 0, 0.0)));
        assert_eq!(timing.snapshot(), None);
    }

    #[test]
    fn reset_discards_completed_match_measurements() {
        let mut timing = FrameTimingAccumulator::new(true);
        timing.reset_for_match(1);
        timing.record_frame(sample(40.0, 20.0, 2, 12.0));
        assert_eq!(timing.snapshot().unwrap().generation, 1);
        timing.reset_for_match(2);
        assert_eq!(timing.generation(), Some(2));
        assert_eq!(timing.snapshot(), None);
    }

    #[test]
    fn runtime_publication_is_latest_snapshot_and_not_per_frame() {
        let mut timing = FrameTiming::enabled();
        timing.begin_frame(3, 0.0);
        timing.begin_fixed_tick(1.0);
        timing.end_fixed_tick(4.0);
        timing.end_frame(10.0, false);
        assert!(timing.take_publication().is_some());

        timing.begin_frame(3, 20.0);
        timing.end_frame(30.0, false);
        assert_eq!(timing.take_publication(), None);

        timing.begin_frame(3, 200.0);
        timing.end_frame(220.0, false);
        let published = timing.take_publication().unwrap();
        assert_eq!(published.rendered_frames, 3);
        // The third frame took 20 ms without fixed ticks; the earlier
        // 10 ms frame's fixed work must not be paired with this maximum.
        assert_eq!(published.slowest_frame.total_main_app_ms, 20.0);
        assert_eq!(published.slowest_frame.fixed_tick_count, 0);
        assert_eq!(published.fixed_tick_count, 1);
        // A later GameOver frame has no active timing span, so the completed
        // match summary remains available for its final ECS publication.
        timing.end_frame(500.0, true);
        assert_eq!(timing.published(), Some(published));
    }
}
