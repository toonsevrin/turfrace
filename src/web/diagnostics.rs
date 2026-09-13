//! Opt-in, read-only ECS diagnostics for browser performance captures.
//!
//! The JavaScript bridge is deliberately inert unless `?performance` is in the
//! page URL. Nothing in this module changes simulation state.

use bevy::prelude::*;

use crate::{
    app_state::AppState,
    match_game::{Competitor, CompetitorKind, MatchSession},
};

#[derive(Clone, Debug, PartialEq)]
pub struct DiagnosticsSnapshot {
    pub app_state: String,
    pub purpose: Option<String>,
    pub phase: Option<String>,
    pub elapsed_seconds: Option<f32>,
    pub humans: usize,
    pub npcs: usize,
    pub ready: Option<bool>,
}

/// Build a snapshot from ECS values. Kept independent of wasm so the shape and
/// roster accounting can be covered by native tests.
pub fn snapshot<'a, I>(
    app_state: &AppState,
    session: Option<&MatchSession>,
    competitors: I,
    ready: Option<bool>,
) -> DiagnosticsSnapshot
where
    I: IntoIterator<Item = &'a Competitor>,
{
    let (humans, npcs) = competitors
        .into_iter()
        .fold((0, 0), |(humans, npcs), competitor| match competitor.kind {
            CompetitorKind::Human => (humans + 1, npcs),
            CompetitorKind::Npc => (humans, npcs + 1),
        });
    DiagnosticsSnapshot {
        app_state: format!("{app_state:?}"),
        purpose: session.map(|session| format!("{:?}", session.purpose)),
        phase: session.map(|session| format!("{:?}", session.phase)),
        elapsed_seconds: session.map(|session| session.elapsed_seconds),
        humans,
        npcs,
        ready,
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
const diagnosticsEnabled = new URLSearchParams(window.location.search).has("performance");
export function turfrace_diagnostics_enabled() {
    return diagnosticsEnabled;
}

// Rust sends frame timing only when its <=5Hz publication slot advances. Keep
// the latest bounded numeric object while the ECS metadata continues updating
// every frame, rather than allocating a timing record every frame.
let latestFrameTiming = null;

export function turfrace_publish_diagnostics(
    appState, purpose, phase, elapsedSeconds, humans, npcs, ready, readyPresent,
    frameTimingReset, frameTimingPresent, generation, renderedFrames,
    totalMainAppMsSum, fixedMsSum, fixedTickCount, remainderMsSum,
    over16_67ms, over33_33ms, maxFixedTicksPerFrame,
    slowestTotalMainAppMs, slowestFixedMs, slowestFixedTickCount,
    slowestMaxFixedTickMs, slowestRemainderMs, slowestFrameNumber
) {
    if (!turfrace_diagnostics_enabled()) return;
    if (frameTimingReset) latestFrameTiming = null;
    if (frameTimingPresent) {
        latestFrameTiming = {
            generation,
            rendered_frames: renderedFrames,
            total_main_app_ms_sum: totalMainAppMsSum,
            fixed_ms_sum: fixedMsSum,
            fixed_tick_count: fixedTickCount,
            remainder_ms_sum: remainderMsSum,
            over16_67ms,
            over33_33ms,
            max_fixed_ticks_per_frame: maxFixedTicksPerFrame,
            slowest_total_main_app_ms: slowestTotalMainAppMs,
            slowest_fixed_ms: slowestFixedMs,
            slowest_fixed_tick_count: slowestFixedTickCount,
            slowest_max_fixed_tick_ms: slowestMaxFixedTickMs,
            slowest_remainder_ms: slowestRemainderMs,
            slowest_frame_number: slowestFrameNumber,
        };
    }
    const diagnostics = {
        app_state: appState,
        purpose: purpose === "" ? null : purpose,
        phase: phase === "" ? null : phase,
        elapsed_seconds: elapsedSeconds,
        humans,
        npcs,
    };
    if (readyPresent) diagnostics.ready = ready;
    if (latestFrameTiming) diagnostics.frame_timing = latestFrameTiming;
    window.__turfraceDiagnostics = diagnostics;
}
"#)]
extern "C" {
    fn turfrace_diagnostics_enabled() -> bool;
    #[allow(clippy::too_many_arguments)]
    fn turfrace_publish_diagnostics(
        app_state: &str,
        purpose: &str,
        phase: &str,
        elapsed_seconds: f32,
        humans: u32,
        npcs: u32,
        ready: bool,
        ready_present: bool,
        frame_timing_reset: bool,
        frame_timing_present: bool,
        generation: f64,
        rendered_frames: f64,
        total_main_app_ms_sum: f64,
        fixed_ms_sum: f64,
        fixed_tick_count: f64,
        remainder_ms_sum: f64,
        over16_67ms: f64,
        over33_33ms: f64,
        max_fixed_ticks_per_frame: f64,
        slowest_total_main_app_ms: f64,
        slowest_fixed_ms: f64,
        slowest_fixed_tick_count: f64,
        slowest_max_fixed_tick_ms: f64,
        slowest_remainder_ms: f64,
        slowest_frame_number: f64,
    );
}

#[cfg(target_arch = "wasm32")]
pub fn enabled() -> bool {
    turfrace_diagnostics_enabled()
}

#[cfg(target_arch = "wasm32")]
pub fn publish_diagnostics(
    state: Res<State<AppState>>,
    session: Option<Res<MatchSession>>,
    competitors: Query<&Competitor>,
    generation: Option<Res<crate::match_game::MatchGeneration>>,
    presentation_ready: Option<Res<crate::match_game::PresentationReady>>,
    frame_timing: Option<ResMut<crate::web::frame_timing::FrameTiming>>,
) {
    if !turfrace_diagnostics_enabled() {
        return;
    }
    let Some(session) = session.as_deref() else {
        return;
    };
    let ready = generation
        .zip(presentation_ready)
        .map(|(generation, ready)| ready.0 == Some(generation.0));
    let snapshot = snapshot(state.get(), Some(session), competitors.iter(), ready);
    let (frame_timing_reset, frame_timing_values) = frame_timing
        .map(|mut timing| (timing.take_reset_notice(), timing.take_publication()))
        .unwrap_or((false, None));
    let frame_timing_present = frame_timing_values.is_some();
    let values =
        frame_timing_values.unwrap_or_else(|| crate::web::frame_timing::FrameTimingSnapshot {
            generation: 0,
            rendered_frames: 0,
            slowest_frame_number: 0,
            total_main_app_ms_sum: 0.0,
            fixed_ms_sum: 0.0,
            fixed_tick_count: 0,
            remainder_ms_sum: 0.0,
            over16_67ms: 0,
            over33_33ms: 0,
            max_fixed_ticks_per_frame: 0,
            slowest_frame: crate::web::frame_timing::FrameTimingSample {
                total_main_app_ms: 0.0,
                fixed_ms: 0.0,
                fixed_tick_count: 0,
                max_fixed_tick_ms: 0.0,
                remainder_ms: 0.0,
            },
        });
    turfrace_publish_diagnostics(
        &snapshot.app_state,
        snapshot.purpose.as_deref().unwrap_or(""),
        snapshot.phase.as_deref().unwrap_or(""),
        snapshot.elapsed_seconds.unwrap_or(0.0),
        snapshot.humans as u32,
        snapshot.npcs as u32,
        snapshot.ready.unwrap_or(false),
        snapshot.ready.is_some(),
        frame_timing_reset,
        frame_timing_present,
        values.generation as f64,
        values.rendered_frames as f64,
        values.total_main_app_ms_sum,
        values.fixed_ms_sum,
        values.fixed_tick_count as f64,
        values.remainder_ms_sum,
        values.over16_67ms as f64,
        values.over33_33ms as f64,
        values.max_fixed_ticks_per_frame as f64,
        values.slowest_frame.total_main_app_ms,
        values.slowest_frame.fixed_ms,
        values.slowest_frame.fixed_tick_count as f64,
        values.slowest_frame.max_fixed_tick_ms,
        values.slowest_frame.remainder_ms,
        values.slowest_frame_number as f64,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::CompetitorId;
    use crate::match_game::{MatchPhase, MatchPurpose};

    fn competitor(kind: CompetitorKind, id: u8) -> Competitor {
        Competitor {
            id: CompetitorId(id),
            display_name: format!("player-{id}"),
            kind,
            color_id: id,
            pattern_id: 0,
        }
    }

    #[test]
    fn snapshot_reports_debug_state_session_and_ecs_roster() {
        let session = MatchSession {
            purpose: MatchPurpose::Playable,
            phase: MatchPhase::Running,
            elapsed_seconds: 12.5,
            ..default()
        };
        let competitors = [
            competitor(CompetitorKind::Human, 0),
            competitor(CompetitorKind::Npc, 1),
            competitor(CompetitorKind::Npc, 2),
        ];
        assert_eq!(
            snapshot(
                &AppState::Playing,
                Some(&session),
                competitors.iter(),
                Some(true)
            ),
            DiagnosticsSnapshot {
                app_state: "Playing".into(),
                purpose: Some("Playable".into()),
                phase: Some("Running".into()),
                elapsed_seconds: Some(12.5),
                humans: 1,
                npcs: 2,
                ready: Some(true),
            }
        );
    }

    #[test]
    fn snapshot_allows_missing_session_and_readiness() {
        let competitor = competitor(CompetitorKind::Human, 0);
        let result = snapshot(&AppState::Lobby, None, [&competitor], None);
        assert_eq!(result.app_state, "Lobby");
        assert_eq!(result.purpose, None);
        assert_eq!(result.phase, None);
        assert_eq!(result.elapsed_seconds, None);
        assert_eq!((result.humans, result.npcs, result.ready), (1, 0, None));
    }
}
