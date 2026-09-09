//! Deterministic native performance benchmark harness.
//!
//! The harness runs the production renderer in a real winit event loop. Its
//! frame samples are wall-clock intervals between `First` schedule passes;
//! they include event-loop, render submission, and present waiting, but are not
//! CPU or GPU timings.

use std::{
    env, fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use bevy::{
    app::AppExit,
    asset::{AssetMetaCheck, AssetPlugin},
    prelude::*,
    time::TimeUpdateStrategy,
    window::{PresentMode, WindowResolution},
};
use serde_json::json;
use turfrace::{
    app_state::{AppShellPlugin, AppState},
    camera::{PlayerCamera, ViewportSubject},
    config::GameConfig,
    match_game::{
        MatchGeneration, MatchPurpose, MatchSession, MatchSpec, MatchStatistics, PresentationReady,
        RosterDescriptor, SimulationPlugin, start_simulation,
    },
    presentation::PresentationPlugin,
};

const READINESS_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_READINESS_UPDATES: u32 = 6_000;
const MATCH_SEED: u64 = 0x5eed_cafe;
const NPC_ROSTER_SEED: u64 = 0x5eed_beef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scenario {
    Idle,
    CaptureHeavy,
}

impl Scenario {
    fn parse(value: &str) -> Self {
        match value {
            "idle" => Self::Idle,
            "capture-heavy" => Self::CaptureHeavy,
            _ => panic!("--scenario must be idle or capture-heavy"),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::CaptureHeavy => "capture-heavy",
        }
    }
}

#[derive(Resource)]
struct BenchmarkOptions {
    views: usize,
    scenario: Scenario,
    warmup_frames: u32,
    measured_frames: u32,
    width: u32,
    height: u32,
    output: PathBuf,
}

/// All lifecycle timing state lives in this resource rather than in a loop
/// around `App::update`. That distinction is important: only the winit runner
/// can account for its event processing and presentation wait.
#[derive(Resource)]
struct BenchmarkRun {
    startup: Instant,
    last_first: Option<Instant>,
    readiness: Option<Instant>,
    configured_at: Option<Instant>,
    first_first_ms: Option<f64>,
    readiness_updates: u32,
    warmup_remaining: u32,
    measured_samples_ms: Vec<f64>,
    fixed_before: Option<u64>,
    captures_before: Option<u32>,
    configured: bool,
    finished: bool,
}

impl BenchmarkRun {
    fn new(options: &BenchmarkOptions, startup: Instant) -> Self {
        Self {
            startup,
            last_first: None,
            readiness: None,
            configured_at: None,
            first_first_ms: None,
            readiness_updates: 0,
            warmup_remaining: options.warmup_frames,
            measured_samples_ms: Vec::with_capacity(options.measured_frames as usize),
            fixed_before: None,
            captures_before: None,
            configured: false,
            finished: false,
        }
    }
}

#[derive(Resource, Default)]
struct BenchmarkCounter {
    fixed_updates: u64,
}

fn count_fixed_update(mut counter: ResMut<BenchmarkCounter>) {
    counter.fixed_updates += 1;
}

fn configure_benchmark(world: &mut World) {
    let (views, scenario) = {
        let options = world.resource::<BenchmarkOptions>();
        (options.views, options.scenario)
    };
    // Every view count runs the same twelve seeded NPC controllers. The first
    // N are classified as local subjects only so the presentation creates N
    // cameras; no device adapter or HumanController is attached.
    let roster = vec![RosterDescriptor::Npc; 12];
    let config = GameConfig {
        player_speed: if scenario == Scenario::Idle {
            0.0
        } else {
            GameConfig::default().player_speed
        },
        ..default()
    };
    let mut spec = MatchSpec::from_config(MATCH_SEED, roster, &config);
    spec.npc_roster_seed = NPC_ROSTER_SEED;
    spec.npc_difficulty = turfrace::npc::NpcDifficulty::Normal;
    spec.purpose = MatchPurpose::Playable;
    spec.countdown_ticks = 0;
    start_simulation(world, &spec);
    for mut competitor in world
        .query::<&mut turfrace::match_game::Competitor>()
        .iter_mut(world)
    {
        if usize::from(competitor.id.0) < views {
            competitor.kind = turfrace::match_game::CompetitorKind::Human;
        }
    }
    // Loading simulation generations get the production trail-pipeline warmup.
    // The benchmark owns the simulation directly, so the shell match adapter is
    // intentionally not installed and cannot replace this exact MatchSpec.
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::MatchLoading);
}

/// Drive setup and sampling from `First`, once per real runner iteration.
fn drive_benchmark(world: &mut World) {
    let now = Instant::now();
    let (previous_first, startup, configured, readiness, finished) = {
        let mut run = world.resource_mut::<BenchmarkRun>();
        let previous_first = run.last_first.replace(now);
        if run.first_first_ms.is_none() {
            run.first_first_ms = Some(now.duration_since(run.startup).as_secs_f64() * 1000.0);
        }
        (
            previous_first,
            run.startup,
            run.configured,
            run.readiness,
            run.finished,
        )
    };
    if finished {
        return;
    }

    if now.duration_since(startup) >= READINESS_TIMEOUT {
        fail_benchmark(world, "presentation readiness timed out");
        return;
    }

    if !configured {
        if *world.resource::<State<AppState>>().get() != AppState::Home {
            return;
        }
        configure_benchmark(world);
        let mut run = world.resource_mut::<BenchmarkRun>();
        run.configured = true;
        run.configured_at = Some(now);
        return;
    }

    if readiness.is_none() {
        if presentation_is_ready(world) {
            // Only leave MatchLoading after the render world has acknowledged
            // this exact generation and its required pipeline witnesses.
            world
                .resource_mut::<NextState<AppState>>()
                .set(AppState::Playing);
            let warmup_frames = world.resource::<BenchmarkOptions>().warmup_frames;
            if warmup_frames == 0 {
                let fixed_updates = world.resource::<BenchmarkCounter>().fixed_updates;
                let captures = capture_count(world);
                let mut run = world.resource_mut::<BenchmarkRun>();
                run.readiness = Some(now);
                run.warmup_remaining = 0;
                run.fixed_before = Some(fixed_updates);
                run.captures_before = Some(captures);
            } else {
                let mut run = world.resource_mut::<BenchmarkRun>();
                run.readiness = Some(now);
                run.warmup_remaining = warmup_frames;
            }
            return;
        }
        let updates = {
            let mut run = world.resource_mut::<BenchmarkRun>();
            run.readiness_updates = run.readiness_updates.saturating_add(1);
            run.readiness_updates
        };
        if updates >= MAX_READINESS_UPDATES {
            fail_benchmark(world, "presentation readiness update limit exceeded");
        }
        return;
    }

    // The production render witness is a one-time generation handshake. Once
    // it has succeeded, keep that acknowledgement latched while sampling;
    // leaving MatchLoading removes only the temporary trail witness and must
    // not stop deterministic simulation ticks for the benchmark.
    let generation = world.resource::<MatchGeneration>().0;
    if world.resource::<PresentationReady>().0 != Some(generation) {
        world.resource_mut::<PresentationReady>().0 = Some(generation);
    }

    let warmup_remaining = world.resource::<BenchmarkRun>().warmup_remaining;
    if warmup_remaining > 0 {
        let fixed_updates = world.resource::<BenchmarkCounter>().fixed_updates;
        let captures = capture_count(world);
        let mut run = world.resource_mut::<BenchmarkRun>();
        run.warmup_remaining -= 1;
        if run.warmup_remaining == 0 {
            run.fixed_before = Some(fixed_updates);
            run.captures_before = Some(captures);
        }
        return;
    }

    if let Some(previous_first) = previous_first {
        let elapsed_ms = now.duration_since(previous_first).as_secs_f64() * 1000.0;
        let measured_frames = world.resource::<BenchmarkOptions>().measured_frames;
        let measured = {
            let mut run = world.resource_mut::<BenchmarkRun>();
            run.measured_samples_ms.push(elapsed_ms);
            run.measured_samples_ms.len() >= measured_frames as usize
        };
        if measured {
            finish_benchmark(world, None);
        }
    }
}

fn percentile(values: &[f64], quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() - 1) as f64 * quantile).round() as usize;
    sorted[index]
}

fn arguments() -> (usize, Scenario, u32, u32, u32, u32, PathBuf) {
    let mut views = None;
    let mut scenario = Scenario::Idle;
    let mut warmup = 120;
    let mut frames = 600;
    let mut width = 1920;
    let mut height = 1080;
    let mut output = PathBuf::from("target/performance/benchmark.json");
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        let mut value = || {
            args.next()
                .unwrap_or_else(|| panic!("{argument} needs a value"))
        };
        match argument.as_str() {
            "--views" => views = Some(value().parse().expect("--views must be an integer")),
            "--scenario" => scenario = Scenario::parse(&value()),
            "--warmup" => warmup = value().parse().expect("--warmup must be an integer"),
            "--frames" => frames = value().parse().expect("--frames must be an integer"),
            "--width" => width = value().parse().expect("--width must be an integer"),
            "--height" => height = value().parse().expect("--height must be an integer"),
            "--output" => output = PathBuf::from(value()),
            "--help" | "-h" => {
                println!(
                    "usage: performance_benchmark --views 1|2|4|8 [--scenario idle|capture-heavy] [--warmup N] [--frames N] [--width PX] [--height PX] [--output PATH]"
                );
                std::process::exit(0);
            }
            _ => panic!("unknown argument `{argument}`; use --help"),
        }
    }
    let views = views.expect("--views is required");
    assert!(
        matches!(views, 1 | 2 | 4 | 8),
        "--views must be one of 1, 2, 4, or 8"
    );
    assert!(
        width > 0 && height > 0,
        "viewport dimensions must be positive"
    );
    assert!(frames > 0, "--frames must be positive");
    (views, scenario, warmup, frames, width, height, output)
}

fn main() {
    let (views, scenario, warmup_frames, measured_frames, width, height, output) = arguments();
    let startup = Instant::now();
    let options = BenchmarkOptions {
        views,
        scenario,
        warmup_frames,
        measured_frames,
        width,
        height,
        output,
    };
    App::new()
        .insert_resource(TimeUpdateStrategy::FixedTimesteps(1))
        .insert_resource(BenchmarkRun::new(&options, startup))
        .insert_resource(options)
        .insert_resource(BenchmarkCounter::default())
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: format!("Turfrace Performance Benchmark ({views} views)"),
                        resolution: WindowResolution::new(width, height),
                        present_mode: PresentMode::Immediate,
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: format!("{}/assets", env!("CARGO_MANIFEST_DIR")),
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                }),
        )
        .add_plugins((AppShellPlugin, SimulationPlugin, PresentationPlugin))
        .add_systems(First, drive_benchmark)
        .add_systems(FixedUpdate, count_fixed_update)
        .run();
}

fn finish_benchmark(world: &mut World, error: Option<&str>) {
    let (views, scenario, warmup_frames, measured_frames, width, height, output) = {
        let options = world.resource::<BenchmarkOptions>();
        (
            options.views,
            options.scenario,
            options.warmup_frames,
            options.measured_frames,
            options.width,
            options.height,
            options.output.clone(),
        )
    };
    let (
        startup,
        readiness,
        configured_at,
        first_first_ms,
        readiness_updates,
        fixed_before,
        captures_before,
        samples,
    ) = {
        let run = world.resource::<BenchmarkRun>();
        (
            run.startup,
            run.readiness,
            run.configured_at,
            run.first_first_ms,
            run.readiness_updates,
            run.fixed_before,
            run.captures_before,
            run.measured_samples_ms.clone(),
        )
    };
    let cameras = camera_metrics(world);
    let canonical_cameras = canonical_camera_verification(world, views, cameras.len());
    let controller_workload = controller_workload_verification(world, views);
    let startup_to_first_first_ms = first_first_ms.unwrap_or_default();
    let startup_to_readiness_ms = readiness.map_or(0.0, |ready| {
        ready.duration_since(startup).as_secs_f64() * 1000.0
    });
    let readiness_wait_ms = readiness
        .zip(configured_at)
        .map_or(0.0, |(ready, configured)| {
            ready.duration_since(configured).as_secs_f64() * 1000.0
        });
    let fixed_updates = fixed_before.map_or(0, |before| {
        world
            .resource::<BenchmarkCounter>()
            .fixed_updates
            .saturating_sub(before)
    });
    let captures = captures_before.map_or(0, |before| capture_count(world).saturating_sub(before));
    let scenario = scenario.as_str();
    let status = if error.is_some() || !canonical_cameras || !controller_workload {
        "failed"
    } else {
        "ok"
    };
    let timing = if error.is_some() || samples.is_empty() {
        serde_json::Value::Null
    } else {
        json!({
            "source": "native_process_wall_clock_first_to_first",
            "sample_count": samples.len(),
            "median_ms": percentile(&samples, 0.50),
            "p95_ms": percentile(&samples, 0.95),
            "max_ms": samples.iter().copied().fold(0.0, f64::max),
            "min_ms": samples.iter().copied().fold(f64::INFINITY, f64::min),
            "equivalent_p95_fps": 1000.0 / percentile(&samples, 0.95).max(f64::EPSILON),
            "fixed_updates": fixed_updates,
            "fixed_update_cpu_ms": null,
            "fixed_update_cpu_reason": "No fixed-system profiler is exposed; samples include scheduling and rendering."
        })
    };
    let result = json!({
        "schema_version": 1,
        "status": status,
        "error": error,
        "views": views,
        "scenario": scenario,
        "canvas": { "width": width, "height": height },
        "warmup_frames": warmup_frames,
        "measured_frames": measured_frames,
        "startup": {
            "source": "native_process_wall_clock",
            "startup_to_first_first_ms": startup_to_first_first_ms,
            "startup_to_readiness_ms": startup_to_readiness_ms,
            "readiness_wait_ms": readiness_wait_ms,
            "readiness_updates": readiness_updates
        },
        "timing": timing,
        "simulation": {
            "competitors": world.query_filtered::<Entity, With<turfrace::match_game::Competitor>>().iter(world).count(),
            "match_seed": MATCH_SEED,
            "npc_roster_seed": NPC_ROSTER_SEED,
            "npc_difficulty": "normal",
            "npc_controllers": world.query_filtered::<Entity, With<turfrace::npc::NpcController>>().iter(world).count(),
            "human_controllers": world.query_filtered::<Entity, With<turfrace::input::HumanController>>().iter(world).count(),
            "human_competitors": world
                .query::<&turfrace::match_game::Competitor>()
                .iter(world)
                .filter(|competitor| competitor.kind == turfrace::match_game::CompetitorKind::Human)
                .count(),
            "player_speed": world.resource::<GameConfig>().player_speed,
            "match_elapsed_seconds": world.get_resource::<MatchSession>().map_or(0.0, |session| session.elapsed_seconds),
            "captures_observed": captures,
            "capture_activity_verified": scenario == "idle" || captures > 0
        },
        "camera_count": cameras.len(),
        "cameras": cameras,
        "validation": {
            "canonical_view_cameras": canonical_cameras,
            "controller_workload": controller_workload,
            "browser_verified": false,
            "hardware_verified": false,
            "memory_verified": false,
            "software_renderer_possible": true,
            "notes": "First-to-First intervals include event-loop, render submission, and present wait; CPU/GPU, browser, memory, and physical-device metrics are not measured."
        }
    });
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create benchmark output directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&result).expect("serialize benchmark report"),
    )
    .expect("write benchmark report");
    println!("PERFORMANCE_BENCHMARK {}", output.display());
    world.resource_mut::<BenchmarkRun>().finished = true;
    world.write_message(if status == "ok" {
        AppExit::Success
    } else {
        AppExit::error()
    });
}

fn fail_benchmark(world: &mut World, reason: &str) {
    if world.resource::<BenchmarkRun>().finished {
        return;
    }
    eprintln!("performance benchmark failed: {reason}");
    finish_benchmark(world, Some(reason));
}

fn presentation_is_ready(world: &World) -> bool {
    world.resource::<BenchmarkRun>().configured
        && world.resource::<PresentationReady>().0 == Some(world.resource::<MatchGeneration>().0)
}

fn capture_count(world: &mut World) -> u32 {
    world
        .query::<&MatchStatistics>()
        .iter(world)
        .map(|stats| stats.captures_completed)
        .sum()
}

fn camera_metrics(world: &mut World) -> Vec<serde_json::Value> {
    let snapshots: Vec<_> = {
        let mut query = world.query::<(&PlayerCamera, &Camera)>();
        query
            .iter(world)
            .filter_map(|(player, camera)| {
                camera.is_active.then_some(()).and_then(|()| {
                    camera
                        .viewport
                        .clone()
                        .map(|viewport| (player.slot, player.subject, viewport))
                })
            })
            .collect()
    };
    let mut metrics = Vec::with_capacity(snapshots.len());
    for (slot, subject_entity, viewport) in snapshots {
        let subject = world.get::<ViewportSubject>(subject_entity);
        metrics.push(json!({
            "slot": slot,
            "subject_x": subject.map_or(0.0, |value| value.position.x),
            "subject_y": subject.map_or(0.0, |value| value.position.y),
            "x": viewport.physical_position.x,
            "y": viewport.physical_position.y,
            "width": viewport.physical_size.x,
            "height": viewport.physical_size.y,
            "pixels": u64::from(viewport.physical_size.x) * u64::from(viewport.physical_size.y),
            "cpu_ms": null,
            "gpu_ms": null
        }));
    }
    metrics.sort_by_key(|metric| metric["slot"].as_u64().unwrap_or_default());
    metrics
}

fn canonical_camera_verification(
    world: &mut World,
    expected_views: usize,
    actual_views: usize,
) -> bool {
    if actual_views != expected_views {
        return false;
    }
    let snapshots: Vec<_> = {
        let mut query = world.query::<(&PlayerCamera, &Camera)>();
        query
            .iter(world)
            .map(|(player, camera)| {
                (
                    player.slot,
                    player.subject,
                    camera.is_active,
                    camera.viewport.is_some(),
                )
            })
            .collect()
    };
    let mut slots = Vec::new();
    let mut subjects = Vec::new();
    for (slot, subject, active, has_viewport) in snapshots {
        if !active || !has_viewport || slots.contains(&slot) {
            return false;
        }
        let Some(competitor) = world.get::<turfrace::match_game::Competitor>(subject) else {
            return false;
        };
        if competitor.kind != turfrace::match_game::CompetitorKind::Human
            || world.get::<ViewportSubject>(subject).is_none()
            || subjects.contains(&subject)
        {
            return false;
        }
        slots.push(slot);
        subjects.push(subject);
    }
    slots.sort_unstable();
    slots == (0..expected_views as u8).collect::<Vec<_>>()
}

/// The benchmark presents the first N NPC-controlled entities as local views.
/// This verifies that presentation classification did not silently attach an
/// input device or replace the constant seeded controller workload.
fn controller_workload_verification(world: &mut World, expected_views: usize) -> bool {
    let mut competitors = Vec::new();
    let mut human_ids = Vec::new();
    let mut npc_controller_count = 0;
    let mut human_controller_count = 0;
    let mut query = world.query::<(
        Entity,
        &turfrace::match_game::Competitor,
        Option<&turfrace::npc::NpcController>,
        Option<&turfrace::input::HumanController>,
    )>();
    for (entity, competitor, npc_controller, human_controller) in query.iter(world) {
        competitors.push(entity);
        npc_controller_count += usize::from(npc_controller.is_some());
        human_controller_count += usize::from(human_controller.is_some());
        if competitor.kind == turfrace::match_game::CompetitorKind::Human {
            human_ids.push(competitor.id.0);
        }
    }
    human_ids.sort_unstable();
    competitors.len() == 12
        && npc_controller_count == 12
        && human_controller_count == 0
        && human_ids == (0..expected_views as u8).collect::<Vec<_>>()
}
