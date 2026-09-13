//! Sustained, renderer-free production simulation timing. Includes snapshot cost.
//! Run a release build: debug timings do not represent browser performance.
//!
//! The default output is unchanged. Opt in to schedule phase diagnostics with
//! `--phase-timing`; that mode reports mean/p95/max cost and 16.67ms overruns for
//! every `MatchSystemSet` and an isolated `HeadlessMatch::snapshot` call. For example:
//!
//! ```text
//! CARGO_BUILD_JOBS=2 cargo run --release --no-default-features \
//!   --example simulation_performance -- --ticks 3600 --window 300 \
//!   --npcs 8 --seed 42 --phase-timing
//! ```
//!
//! `whole_step` is the normal `HeadlessMatch::step` cost and includes its
//! authoritative snapshot. These are fixed-tick CPU diagnostics, not browser FPS.
use std::{
    collections::BTreeMap,
    error::Error,
    time::{Duration, Instant},
};

use bevy::prelude::*;
use serde::Serialize;
use turfrace::{
    config::GameConfig,
    ids::MAX_COMPETITORS,
    match_game::{HeadlessMatch, MatchSpec, MatchSystemSet, RosterDescriptor, SimulationEvents},
};
#[cfg(test)]
use turfrace::{
    match_game::{
        ComparableCompetitor, ComparableSnapshot, Competitor, LifeState, MatchSession,
        MatchStatistics, SimulationClock, SpawnProtection, TerritoryRecord,
    },
    movement::CompetitorMotion,
    npc::NpcController,
};

const MATCH_PHASES: [MatchSystemSet; 13] = [
    MatchSystemSet::PollInput,
    MatchSystemSet::NpcThink,
    MatchSystemSet::BuildSteeringIntent,
    MatchSystemSet::MoveCompetitors,
    MatchSystemSet::ExtendTrails,
    MatchSystemSet::DetectTrailCollisions,
    MatchSystemSet::ResolveDeaths,
    MatchSystemSet::DetectClosures,
    MatchSystemSet::ResolveCaptures,
    MatchSystemSet::ResolveTerritoryConsequences,
    MatchSystemSet::CheckVictory,
    MatchSystemSet::AdvanceRespawns,
    MatchSystemSet::UpdateRankings,
];

#[derive(Debug)]
struct Options {
    ticks: u64,
    window: u64,
    npcs: usize,
    seed: u64,
    phase_timing: bool,
    cpu_timing: bool,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut result = Self {
            ticks: 3_600,
            window: 300,
            npcs: 8,
            seed: 42,
            phase_timing: false,
            cpu_timing: false,
        };
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            if flag == "--phase-timing" {
                result.phase_timing = true;
                continue;
            }
            if flag == "--cpu-timing" {
                result.cpu_timing = true;
                continue;
            }
            let value = args
                .next()
                .ok_or_else(|| format!("missing value for {flag}"))?;
            match flag.as_str() {
                "--ticks" => result.ticks = value.parse().map_err(|_| "invalid ticks")?,
                "--window" => result.window = value.parse().map_err(|_| "invalid window")?,
                "--npcs" => result.npcs = value.parse().map_err(|_| "invalid NPC count")?,
                "--seed" => result.seed = value.parse().map_err(|_| "invalid seed")?,
                _ => return Err(format!("unknown option {flag}")),
            }
        }
        if !(1..=216_000).contains(&result.ticks)
            || !(1..=3_600).contains(&result.window)
            || !(2..=MAX_COMPETITORS).contains(&result.npcs)
        {
            return Err(format!(
                "ticks must be 1..216000, window 1..3600, NPCs 2..{MAX_COMPETITORS}"
            ));
        }
        Ok(result)
    }
}

#[derive(Serialize)]
struct TimingWindow {
    first_tick: u64,
    last_tick: u64,
    samples: usize,
    mean_ms: f64,
    p95_ms: f64,
    max_ms: f64,
    ticks_over_16_67_ms: usize,
    max_active_trail_length: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    phase_timing: Option<PhaseTimingWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu_timing: Option<CpuTimingWindow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
struct CpuTimingSample {
    tick: u64,
    wall_ms: f64,
    cpu_ms: f64,
}

#[derive(Serialize)]
struct CpuTimingWindow {
    samples: usize,
    mean_ms: f64,
    p95_ms: f64,
    max_ms: f64,
    ticks_over_16_67_ms: usize,
    wall_max_sample: CpuTimingSample,
    wall_over_33_33_cpu_under_16_67: usize,
}

#[derive(Serialize)]
struct PhaseTimingWindow {
    /// The complete `HeadlessMatch::step`, including its snapshot.
    whole_step: TimingStats,
    /// An isolated public `HeadlessMatch::snapshot` call, not a browser frame.
    headless_snapshot: TimingStats,
    match_system_sets: BTreeMap<String, TimingStats>,
}

#[derive(Serialize)]
struct TimingStats {
    samples: usize,
    mean_ms: f64,
    p95_ms: f64,
    max_ms: f64,
    ticks_over_16_67_ms: usize,
}

#[cfg(target_os = "linux")]
fn process_cpu_now() -> Result<Duration, String> {
    let mut timestamp = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // `timestamp` is initialized and points to writable storage for the C API.
    let return_code =
        unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut timestamp) };
    if return_code != 0 {
        return Err(format!(
            "clock_gettime(CLOCK_PROCESS_CPUTIME_ID) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if timestamp.tv_sec < 0 || timestamp.tv_nsec < 0 || timestamp.tv_nsec >= 1_000_000_000 {
        return Err(format!(
            "clock_gettime(CLOCK_PROCESS_CPUTIME_ID) returned invalid timespec: tv_sec={} tv_nsec={}",
            timestamp.tv_sec, timestamp.tv_nsec
        ));
    }
    Ok(Duration::new(
        timestamp.tv_sec as u64,
        timestamp.tv_nsec as u32,
    ))
}

#[cfg(not(target_os = "linux"))]
fn process_cpu_now() -> Result<Duration, String> {
    Err("--cpu-timing is unsupported on this OS; process CPU timing requires Linux".into())
}

fn cpu_delta_ms(start: Duration, end: Duration) -> Result<f64, String> {
    end.checked_sub(start)
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .ok_or_else(|| "process CPU clock moved backwards during a simulation step".into())
}

fn wall_max_sample(samples: &[CpuTimingSample]) -> CpuTimingSample {
    assert!(
        !samples.is_empty(),
        "CPU timing windows must contain samples"
    );
    let mut maximum = samples[0];
    for &sample in &samples[1..] {
        // Keep the first sample on an equal wall-time tie, so its CPU value
        // remains paired with the selected wall measurement.
        if sample.wall_ms.total_cmp(&maximum.wall_ms).is_gt() {
            maximum = sample;
        }
    }
    maximum
}

struct CpuTimingSamples {
    samples: Vec<CpuTimingSample>,
}

impl CpuTimingSamples {
    fn new(window: usize) -> Self {
        Self {
            samples: Vec::with_capacity(window),
        }
    }

    fn add(&mut self, tick: u64, wall_ms: f64, cpu_ms: f64) {
        self.samples.push(CpuTimingSample {
            tick,
            wall_ms,
            cpu_ms,
        });
    }

    fn summarize(&self) -> CpuTimingWindow {
        assert!(
            !self.samples.is_empty(),
            "CPU timing windows must contain samples"
        );
        let mut cpu_times = self
            .samples
            .iter()
            .map(|sample| sample.cpu_ms)
            .collect::<Vec<_>>();
        let stats = summarize_stats(&mut cpu_times);
        CpuTimingWindow {
            samples: stats.samples,
            mean_ms: stats.mean_ms,
            p95_ms: stats.p95_ms,
            max_ms: stats.max_ms,
            ticks_over_16_67_ms: stats.ticks_over_16_67_ms,
            wall_max_sample: wall_max_sample(&self.samples),
            wall_over_33_33_cpu_under_16_67: self
                .samples
                .iter()
                .filter(|sample| sample.wall_ms > 1000.0 / 30.0 && sample.cpu_ms <= 1000.0 / 60.0)
                .count(),
        }
    }
}

fn summarize_stats(times: &mut [f64]) -> TimingStats {
    if times.is_empty() {
        return TimingStats {
            samples: 0,
            mean_ms: 0.0,
            p95_ms: 0.0,
            max_ms: 0.0,
            ticks_over_16_67_ms: 0,
        };
    }
    times.sort_by(f64::total_cmp);
    TimingStats {
        samples: times.len(),
        mean_ms: times.iter().sum::<f64>() / times.len() as f64,
        p95_ms: times[(times.len() * 95).div_ceil(100) - 1],
        max_ms: times[times.len() - 1],
        ticks_over_16_67_ms: times.iter().filter(|&&ms| ms > 1000.0 / 60.0).count(),
    }
}

#[cfg(test)]
fn summarize(first_tick: u64, last_tick: u64, times: &mut [f64], max_trail: f32) -> TimingWindow {
    summarize_with_cpu(first_tick, last_tick, times, max_trail, None)
}

fn summarize_with_cpu(
    first_tick: u64,
    last_tick: u64,
    times: &mut [f64],
    max_trail: f32,
    cpu_timing: Option<CpuTimingWindow>,
) -> TimingWindow {
    times.sort_by(f64::total_cmp);
    TimingWindow {
        first_tick,
        last_tick,
        samples: times.len(),
        mean_ms: times.iter().sum::<f64>() / times.len() as f64,
        p95_ms: times[(times.len() * 95).div_ceil(100) - 1],
        max_ms: times[times.len() - 1],
        ticks_over_16_67_ms: times.iter().filter(|&&ms| ms > 1000.0 / 60.0).count(),
        max_active_trail_length: max_trail,
        phase_timing: None,
        cpu_timing,
    }
}

#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum PhaseInstrumentationSet {
    Begin(usize),
    End(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PhaseMarker {
    Begin(usize),
    End(usize),
}

#[derive(Resource)]
struct PhaseTimer {
    starts: [Option<Instant>; MATCH_PHASES.len()],
    last: [Option<f64>; MATCH_PHASES.len()],
    markers: [Option<PhaseMarker>; MATCH_PHASES.len() * 2],
    marker_count: usize,
}

impl Default for PhaseTimer {
    fn default() -> Self {
        Self {
            starts: std::array::from_fn(|_| None),
            last: [None; MATCH_PHASES.len()],
            markers: [None; MATCH_PHASES.len() * 2],
            marker_count: 0,
        }
    }
}

impl PhaseTimer {
    fn begin(&mut self, phase: usize) {
        self.markers[self.marker_count] = Some(PhaseMarker::Begin(phase));
        self.marker_count += 1;
        self.starts[phase] = Some(Instant::now());
    }

    fn end(&mut self, phase: usize) {
        self.markers[self.marker_count] = Some(PhaseMarker::End(phase));
        self.marker_count += 1;
        if let Some(start) = self.starts[phase].take() {
            self.last[phase] = Some(start.elapsed().as_secs_f64() * 1000.0);
        }
    }

    fn take_tick(
        &mut self,
    ) -> (
        [Option<f64>; MATCH_PHASES.len()],
        [Option<PhaseMarker>; MATCH_PHASES.len() * 2],
    ) {
        let last = self.last;
        let markers = self.markers;
        self.last = [None; MATCH_PHASES.len()];
        self.markers = [None; MATCH_PHASES.len() * 2];
        self.marker_count = 0;
        (last, markers)
    }
}

fn instrument_schedule(headless: HeadlessMatch) -> App {
    let mut app: App = headless.into();
    app.insert_resource(PhaseTimer::default());
    for (index, phase) in MATCH_PHASES.into_iter().enumerate() {
        // These are deliberately explicit sets rather than independent
        // before/after constraints. The latter allow every phase's begin and
        // end marker to become runnable in the same scheduling gap.
        app.configure_sets(
            FixedUpdate,
            (
                PhaseInstrumentationSet::Begin(index),
                phase,
                PhaseInstrumentationSet::End(index),
            )
                .chain(),
        );
        if index > 0 {
            app.configure_sets(
                FixedUpdate,
                PhaseInstrumentationSet::End(index - 1)
                    .before(PhaseInstrumentationSet::Begin(index)),
            );
        }
        app.add_systems(
            FixedUpdate,
            (move |mut timer: ResMut<PhaseTimer>| timer.begin(index))
                .in_set(PhaseInstrumentationSet::Begin(index)),
        );
        app.add_systems(
            FixedUpdate,
            // Do not gate the end marker: a terminal CheckVictory system may
            // change MatchSession before its marker runs.
            (move |mut timer: ResMut<PhaseTimer>| timer.end(index))
                .in_set(PhaseInstrumentationSet::End(index)),
        );
    }
    app
}

/// Advances the converted public `HeadlessMatch` app without introducing a
/// production API or timing code. NPC-only matches have no commands to enqueue.
fn run_instrumented_step(
    app: &mut App,
) -> (
    [Option<f64>; MATCH_PHASES.len()],
    [Option<PhaseMarker>; MATCH_PHASES.len() * 2],
) {
    let dt = app.world().resource::<Time<Fixed>>().timestep();
    app.world_mut().resource_mut::<Time<Fixed>>().advance_by(dt);
    app.world_mut().run_schedule(FixedUpdate);
    app.world_mut()
        .resource_mut::<SimulationEvents>()
        .drain()
        .for_each(drop);
    app.world_mut().resource_mut::<PhaseTimer>().take_tick()
}

#[cfg(test)]
fn hash_word(hash: &mut u64, word: u64) {
    *hash ^= word;
    *hash = hash.wrapping_mul(0x1000_0000_01b3);
}

#[cfg(test)]
fn hash_f32(hash: &mut u64, value: f32) {
    hash_word(hash, u64::from(value.to_bits()));
}

#[cfg(test)]
fn territory_fingerprint(map: &turfrace::territory_map::TerritoryMap) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for geometry in std::iter::once(map.arena()).chain(map.territories()) {
        for contour in geometry.contours() {
            hash_word(&mut hash, contour.len() as u64);
            for point in contour {
                hash_word(&mut hash, point.x as u32 as u64);
                hash_word(&mut hash, point.y as u32 as u64);
            }
        }
        hash_word(&mut hash, u64::MAX);
    }
    hash
}

#[cfg(test)]
fn trail_fingerprint(trail: &turfrace::trail::ActiveTrail) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    hash_word(&mut hash, trail.points.len() as u64);
    for point in trail
        .points
        .iter()
        .copied()
        .chain(std::iter::once(trail.head))
    {
        hash_f32(&mut hash, point.x);
        hash_f32(&mut hash, point.y);
    }
    hash_word(&mut hash, trail.cells.len() as u64);
    hash
}

/// Builds the public snapshot shape from the instrumented app for the
/// equivalence regression. The fixture deliberately uses human rosters, so
/// the optional NPC fingerprint is zero just as it is in HeadlessMatch.
#[cfg(test)]
fn snapshot_from_instrumented_app(app: &mut App) -> ComparableSnapshot {
    let world = app.world_mut();
    let (tick, phase, winner) = {
        let session = world.resource::<MatchSession>();
        (
            world.resource::<SimulationClock>().0,
            session.phase,
            session.winner,
        )
    };
    let territory_fingerprint =
        territory_fingerprint(world.resource::<turfrace::territory_map::TerritoryMap>());
    let mut query = world.query::<(
        &Competitor,
        &CompetitorMotion,
        &LifeState,
        &TerritoryRecord,
        &MatchStatistics,
        &SpawnProtection,
        Option<&turfrace::trail::ActiveTrail>,
        Option<&NpcController>,
    )>();
    let mut competitors = query
        .iter(world)
        .map(
            |(competitor, motion, life, territory, stats, protection, trail, npc)| {
                assert!(
                    npc.is_none(),
                    "the equivalence fixture must not contain NPCs"
                );
                ComparableCompetitor {
                    id: competitor.id.0,
                    position: motion.position.to_array(),
                    heading: motion.heading.to_array(),
                    alive: life.is_alive(),
                    territory_area: territory.current_area,
                    kills: stats.kills,
                    kill_streak: stats.kill_streak,
                    deaths: stats.deaths,
                    captures_completed: stats.captures_completed,
                    respawn_remaining: life.respawn_remaining,
                    spawn_protection_remaining: protection.remaining,
                    trail_length: trail.map_or(0.0, |trail| trail.length),
                    trail_fingerprint: trail.map_or(0, trail_fingerprint),
                    npc_fingerprint: 0,
                }
            },
        )
        .collect::<Vec<_>>();
    competitors.sort_by_key(|competitor| competitor.id);
    ComparableSnapshot {
        tick,
        phase: format!("{phase:?}"),
        winner: winner.map(|winner| winner.0),
        competitors,
        territory_fingerprint,
    }
}

struct PhaseSamples {
    snapshots: Vec<f64>,
    phases: [Vec<f64>; MATCH_PHASES.len()],
}

impl PhaseSamples {
    fn new(window: usize) -> Self {
        Self {
            snapshots: Vec::with_capacity(window),
            phases: std::array::from_fn(|_| Vec::with_capacity(window)),
        }
    }

    fn add_snapshot(&mut self, milliseconds: f64) {
        self.snapshots.push(milliseconds);
    }

    fn add_phases(&mut self, timings: [Option<f64>; MATCH_PHASES.len()]) {
        for (samples, timing) in self.phases.iter_mut().zip(timings) {
            if let Some(milliseconds) = timing {
                samples.push(milliseconds);
            }
        }
    }

    fn summarize(&mut self, whole_step: &[f64]) -> PhaseTimingWindow {
        let mut match_system_sets = BTreeMap::new();
        for (phase, samples) in MATCH_PHASES.iter().zip(&mut self.phases) {
            match_system_sets.insert(format!("{phase:?}"), summarize_stats(samples));
        }
        PhaseTimingWindow {
            whole_step: {
                let mut samples = whole_step.to_vec();
                summarize_stats(&mut samples)
            },
            headless_snapshot: summarize_stats(&mut self.snapshots),
            match_system_sets,
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = Options::parse(std::env::args().skip(1))?;
    if options.cpu_timing && !cfg!(target_os = "linux") {
        return Err(
            "--cpu-timing is unsupported on this OS; process CPU timing requires Linux".into(),
        );
    }
    let config = GameConfig::default();
    let mut spec = MatchSpec::from_config(
        options.seed,
        vec![RosterDescriptor::Npc; options.npcs],
        &config,
    );
    spec.countdown_ticks = 0;
    let mut simulation =
        HeadlessMatch::new(spec.clone()).map_err(|e| format!("invalid spec: {e:?}"))?;
    let mut phase_app = if options.phase_timing {
        Some(instrument_schedule(
            HeadlessMatch::new(spec).map_err(|e| format!("invalid spec: {e:?}"))?,
        ))
    } else {
        None
    };
    eprintln!(
        "seed={} NPCs={} ticks={} window={} debug_assertions={} target={}; timing includes headless snapshots, excludes rendering; NOT browser FPS{}",
        options.seed,
        options.npcs,
        options.ticks,
        options.window,
        cfg!(debug_assertions),
        std::env::consts::ARCH,
        if options.phase_timing && options.cpu_timing {
            "; phase timing enabled (deterministic second schedule run); process CPU timing enabled (Linux process-CPU diagnostic, not FPS; clock overhead added)"
        } else if options.phase_timing {
            "; phase timing enabled (deterministic second schedule run)"
        } else if options.cpu_timing {
            "; process CPU timing enabled (Linux process-CPU diagnostic, not FPS; clock overhead added)"
        } else {
            ""
        }
    );
    let mut times = Vec::with_capacity(options.window as usize);
    let mut cpu_samples = options
        .cpu_timing
        .then(|| CpuTimingSamples::new(options.window as usize));
    let mut phase_samples = options
        .phase_timing
        .then(|| PhaseSamples::new(options.window as usize));
    let mut first_tick = 1;
    let mut max_trail = 0.0_f32;
    for tick in 1..=options.ticks {
        let (output, wall_ms, process_cpu_ms) = if options.cpu_timing {
            let process_cpu_start = process_cpu_now()?;
            let wall_start = Instant::now();
            let output = simulation
                .step([])
                .map_err(|e| format!("step rejected: {e:?}"))?;
            let wall_ms = wall_start.elapsed().as_secs_f64() * 1000.0;
            let process_cpu_end = process_cpu_now()?;
            let process_cpu_ms = cpu_delta_ms(process_cpu_start, process_cpu_end)?;
            (output, wall_ms, Some(process_cpu_ms))
        } else {
            let start = Instant::now();
            let output = simulation
                .step([])
                .map_err(|e| format!("step rejected: {e:?}"))?;
            (output, start.elapsed().as_secs_f64() * 1000.0, None)
        };
        times.push(wall_ms);
        if let (Some(samples), Some(process_cpu_ms)) = (cpu_samples.as_mut(), process_cpu_ms) {
            samples.add(tick, wall_ms, process_cpu_ms);
        }
        if let Some(samples) = phase_samples.as_mut() {
            let snapshot_start = Instant::now();
            let _ = simulation.snapshot();
            samples.add_snapshot(snapshot_start.elapsed().as_secs_f64() * 1000.0);
            let (phase_timings, _) = run_instrumented_step(phase_app.as_mut().expect("phase app"));
            samples.add_phases(phase_timings);
        }
        for competitor in output.snapshot.competitors {
            max_trail = max_trail.max(competitor.trail_length);
        }
        if times.len() == options.window as usize || tick == options.ticks {
            let cpu_report = cpu_samples.as_ref().map(CpuTimingSamples::summarize);
            let mut report =
                summarize_with_cpu(first_tick, tick, &mut times, max_trail, cpu_report);
            if let Some(samples) = phase_samples.as_mut() {
                report.phase_timing = Some(samples.summarize(&times));
            }
            println!("{}", serde_json::to_string(&report)?);
            times.clear();
            if let Some(samples) = cpu_samples.as_mut() {
                samples.samples.clear();
            }
            if let Some(samples) = phase_samples.as_mut() {
                samples.snapshots.clear();
                for phase in &mut samples.phases {
                    phase.clear();
                }
            }
            first_tick = tick + 1;
            max_trail = 0.0;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_unknown_flags_and_phase_switch_are_rejected_or_accepted() {
        for args in [
            ["--ticks", "0"],
            ["--window", "0"],
            ["--npcs", "255"],
            ["--ticks", "216001"],
            ["--unknown", "1"],
        ] {
            assert!(Options::parse(args.map(str::to_owned)).is_err());
        }
        assert!(Options::parse(["--ticks".to_owned()]).is_err());
        let options =
            Options::parse(["--phase-timing".to_owned(), "--cpu-timing".to_owned()].into_iter())
                .unwrap();
        assert!(options.phase_timing);
        assert!(options.cpu_timing);
        assert!(!Options::parse(std::iter::empty()).unwrap().cpu_timing);
        assert_eq!(
            Options::parse(["--npcs".to_owned(), MAX_COMPETITORS.to_string()])
                .unwrap()
                .npcs,
            MAX_COMPETITORS
        );
        assert!(Options::parse(["--npcs".to_owned(), (MAX_COMPETITORS + 1).to_string()]).is_err());
    }

    #[test]
    fn nearest_rank_and_single_sample_windows() {
        let report = summarize(
            1,
            20,
            &mut (1..=20).map(f64::from).collect::<Vec<_>>(),
            12.0,
        );
        assert_eq!(report.p95_ms, 19.0);
        assert_eq!(report.mean_ms, 10.5);
        assert_eq!(report.ticks_over_16_67_ms, 4);
        assert!(report.phase_timing.is_none());
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("phase_timing")
        );
        assert_eq!(summarize(1, 1, &mut [3.0], 0.0).p95_ms, 3.0);
    }

    #[test]
    fn cpu_timing_serializes_only_when_enabled() {
        let mut samples = CpuTimingSamples::new(2);
        samples.add(7, 40.0, 9.0);
        let with_cpu = summarize_with_cpu(7, 7, &mut [40.0], 0.0, Some(samples.summarize()));
        let json = serde_json::to_string(&with_cpu).unwrap();
        assert!(json.contains("\"cpu_timing\""));
        assert!(json.contains("\"wall_max_sample\":{\"tick\":7"));
        assert!(
            !serde_json::to_string(&summarize(1, 1, &mut [3.0], 0.0))
                .unwrap()
                .contains("cpu_timing")
        );
    }

    #[test]
    fn cpu_timing_pairs_wall_argmax_and_handles_ties() {
        let samples = [
            CpuTimingSample {
                tick: 3,
                wall_ms: 40.0,
                cpu_ms: 4.0,
            },
            CpuTimingSample {
                tick: 4,
                wall_ms: 40.0,
                cpu_ms: 14.0,
            },
            CpuTimingSample {
                tick: 5,
                wall_ms: 20.0,
                cpu_ms: 20.0,
            },
        ];
        assert_eq!(wall_max_sample(&samples), samples[0]);
        let mut window = CpuTimingSamples::new(samples.len());
        for sample in samples.iter().copied() {
            window.add(sample.tick, sample.wall_ms, sample.cpu_ms);
        }
        let report = window.summarize();
        assert_eq!(report.wall_max_sample, samples[0]);
        assert_eq!(report.wall_over_33_33_cpu_under_16_67, 2);
    }

    #[test]
    fn cpu_clock_errors_are_not_reported_as_zero() {
        assert!(cpu_delta_ms(Duration::from_millis(2), Duration::from_millis(1)).is_err());
    }

    #[test]
    fn cpu_timing_stats_are_nonempty_and_window_bounded() {
        let mut samples = CpuTimingSamples::new(2);
        samples.add(1, 2.0, 2.0);
        samples.add(2, 3.0, 6.0);
        assert!(samples.samples.len() <= 2);
        let report = samples.summarize();
        assert_eq!(report.samples, 2);
        assert_eq!(report.mean_ms, 4.0);
        assert_eq!(report.p95_ms, 6.0);
        assert_eq!(report.max_ms, 6.0);
    }

    #[test]
    fn phase_samples_are_window_bounded_and_report_p95_and_max() {
        let mut samples = PhaseSamples::new(2);
        samples.add_snapshot(4.0);
        samples.add_snapshot(8.0);
        let mut timings = [None; MATCH_PHASES.len()];
        timings[1] = Some(2.0);
        samples.add_phases(timings);
        timings[1] = Some(6.0);
        samples.add_phases(timings);
        assert_eq!(samples.snapshots.len(), 2);
        assert_eq!(samples.phases[1].len(), 2);

        let report = samples.summarize(&[10.0, 20.0]);
        let npc = &report.match_system_sets["NpcThink"];
        assert_eq!(npc.samples, 2);
        assert_eq!(npc.mean_ms, 4.0);
        assert_eq!(npc.p95_ms, 6.0);
        assert_eq!(npc.max_ms, 6.0);
        assert_eq!(npc.ticks_over_16_67_ms, 0);
        assert_eq!(report.headless_snapshot.mean_ms, 6.0);
        assert_eq!(report.headless_snapshot.p95_ms, 8.0);
        assert_eq!(report.headless_snapshot.max_ms, 8.0);
        assert_eq!(report.headless_snapshot.ticks_over_16_67_ms, 0);
        assert_eq!(report.whole_step.mean_ms, 15.0);
        assert_eq!(report.whole_step.max_ms, 20.0);
        assert_eq!(report.whole_step.ticks_over_16_67_ms, 1);
        assert_eq!(summarize_stats(&mut []).samples, 0);
    }

    fn human_spec() -> MatchSpec {
        let config = GameConfig::default();
        let mut spec = MatchSpec::from_config(
            42,
            vec![
                RosterDescriptor::Human {
                    identity: "a".into(),
                    display_name: "A".into(),
                    color_id: 0,
                    pattern_id: 0,
                },
                RosterDescriptor::Human {
                    identity: "b".into(),
                    display_name: "B".into(),
                    color_id: 1,
                    pattern_id: 1,
                },
            ],
            &config,
        );
        spec.countdown_ticks = 0;
        spec
    }

    #[test]
    fn instrumentation_sets_run_in_phase_order() {
        let mut app = instrument_schedule(HeadlessMatch::new(human_spec()).unwrap());
        let (_, markers) = run_instrumented_step(&mut app);
        let expected = (0..MATCH_PHASES.len())
            .flat_map(|phase| [PhaseMarker::Begin(phase), PhaseMarker::End(phase)])
            .collect::<Vec<_>>();
        assert_eq!(markers.into_iter().flatten().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn instrumented_schedule_preserves_headless_snapshots() {
        let spec = human_spec();
        let mut normal = HeadlessMatch::new(spec.clone()).unwrap();
        let mut instrumented = instrument_schedule(HeadlessMatch::new(spec).unwrap());
        for _ in 0..8 {
            let expected = normal.step([]).unwrap().snapshot;
            let (_, markers) = run_instrumented_step(&mut instrumented);
            assert_eq!(
                markers.into_iter().flatten().count(),
                MATCH_PHASES.len() * 2
            );
            assert_eq!(snapshot_from_instrumented_app(&mut instrumented), expected);
        }
    }
}
