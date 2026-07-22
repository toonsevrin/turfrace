//! Deterministic, screenshot-producing visual playtest harness.
//!
//! Run through `./scripts/visual-feedback <scenario>` so the same user-facing
//! frame can be reviewed by a person or an automated agent.

use std::{env, path::PathBuf};

use bevy::{
    app::AppExit,
    asset::{AssetMetaCheck, AssetPlugin},
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    time::TimeUpdateStrategy,
    window::{PresentMode, WindowResolution},
};
use turfrace::{
    app_state::{AppShellPlugin, AppState},
    input::InputDeviceId,
    lobby::{HumanSetup, Lobby, LobbyPlayer, MatchSetup},
    match_game::{MatchPhase, MatchPlugin, MatchSession, start_match},
    presentation::PresentationPlugin,
    ui::{MatchResults, ResultRow},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Home,
    Leaderboard,
    Lobby,
    Match,
    Pause,
    Results,
    Settings,
}

impl Scenario {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "home" => Some(Self::Home),
            "leaderboard" => Some(Self::Leaderboard),
            "lobby" => Some(Self::Lobby),
            "match" => Some(Self::Match),
            "pause" => Some(Self::Pause),
            "results" => Some(Self::Results),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }

    const fn default_capture_frame(self) -> u32 {
        match self {
            Self::Home | Self::Leaderboard | Self::Settings => 30,
            Self::Lobby | Self::Results => 45,
            Self::Match => 360,
            Self::Pause => 240,
        }
    }
}

#[derive(Resource)]
struct Harness {
    scenario: Scenario,
    output: PathBuf,
    capture_frame: u32,
    frame: u32,
    configured: bool,
    screenshot: Option<Entity>,
}

fn main() {
    let (scenario, output, capture_frame, width, height) = arguments();
    info!(
        ?scenario,
        ?output,
        capture_frame,
        "starting visual playtest"
    );

    App::new()
        .insert_resource(TimeUpdateStrategy::FixedTimesteps(1))
        .insert_resource(Harness {
            scenario,
            output,
            capture_frame,
            frame: 0,
            configured: false,
            screenshot: None,
        })
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: format!("Turfrace Visual Playtest — {scenario:?}"),
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
        .add_plugins((AppShellPlugin, MatchPlugin, PresentationPlugin))
        .add_systems(Update, drive_harness)
        .run();
}

fn arguments() -> (Scenario, PathBuf, u32, u32, u32) {
    let mut scenario = Scenario::Match;
    let mut output = None;
    let mut frames = None;
    let mut seconds = None;
    let mut width = 1280;
    let mut height = 720;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--scenario" => {
                let value = args.next().expect("--scenario needs a value");
                scenario =
                    Scenario::parse(&value).unwrap_or_else(|| panic!("unknown scenario `{value}`"));
            }
            "--output" => output = Some(PathBuf::from(args.next().expect("--output needs a path"))),
            "--frames" => {
                let value = args.next().expect("--frames needs a value");
                frames = Some(value.parse::<u32>().expect("--frames must be an integer"));
            }
            "--seconds" => {
                let value = args.next().expect("--seconds needs a value");
                seconds = Some(
                    value
                        .parse::<f32>()
                        .expect("--seconds must be a positive number"),
                );
            }
            "--width" => {
                let value = args.next().expect("--width needs a value");
                width = value.parse::<u32>().expect("--width must be an integer");
            }
            "--height" => {
                let value = args.next().expect("--height needs a value");
                height = value.parse::<u32>().expect("--height must be an integer");
            }
            "--help" | "-h" => {
                println!(
                    "usage: visual_playtest [--scenario home|leaderboard|lobby|match|pause|results|settings] \
                     [--output PATH.png] [--frames N | --seconds N] \
                     [--width PX] [--height PX]"
                );
                std::process::exit(0);
            }
            other => panic!("unknown argument `{other}`; use --help"),
        }
    }
    let output = output.unwrap_or_else(|| {
        PathBuf::from(format!(
            "target/visual-feedback/{}.png",
            format!("{scenario:?}").to_lowercase()
        ))
    });
    assert!(
        frames.is_none() || seconds.is_none(),
        "use only one of --frames or --seconds"
    );
    assert!(
        width >= 320 && height >= 360,
        "viewport is too small to review"
    );
    let capture_frame = frames
        .or_else(|| seconds.map(|seconds| (seconds.max(0.0) * 60.0).ceil().max(1.0) as u32))
        .unwrap_or_else(|| scenario.default_capture_frame());
    (scenario, output, capture_frame, width, height)
}

fn drive_harness(world: &mut World) {
    let screenshot = world.resource::<Harness>().screenshot;
    if let Some(entity) = screenshot {
        if world.get_entity(entity).is_err() {
            world.write_message(AppExit::Success);
        }
        return;
    }

    let state = *world.resource::<State<AppState>>().get();
    if !world.resource::<Harness>().configured {
        if state != AppState::Home {
            return;
        }
        configure_scenario(world);
        world.resource_mut::<Harness>().configured = true;
    }

    let ready = scenario_is_visible(world);
    if !ready {
        return;
    }
    let mut harness = world.resource_mut::<Harness>();
    harness.frame += 1;
    if harness.frame < harness.capture_frame {
        return;
    }
    let output = harness.output.clone();
    let screenshot = world
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(output))
        .id();
    world.resource_mut::<Harness>().screenshot = Some(screenshot);
}

fn configure_scenario(world: &mut World) {
    let scenario = world.resource::<Harness>().scenario;
    match scenario {
        Scenario::Home => {}
        Scenario::Leaderboard => configure_profiles_screen(world, AppState::LocalLeaderboard),
        Scenario::Lobby => configure_lobby(world),
        Scenario::Match | Scenario::Pause => configure_match(world),
        Scenario::Results => configure_results(world),
        Scenario::Settings => configure_profiles_screen(world, AppState::Settings),
    }
}

fn configure_profiles_screen(world: &mut World, state: AppState) {
    let mut profiles = world.resource_mut::<turfrace::profiles::ProfileStore>();
    for name in ["MOUSE ACE", "KEY KID", "RUNE", "NOVA"] {
        profiles.create(name);
    }
    for (index, profile) in profiles.profiles.iter_mut().enumerate() {
        profile.statistics.games_played = 18 - index as u32 * 2;
        profile.statistics.wins = 7_u32.saturating_sub(index as u32);
        profile.statistics.kills = 42 - index as u32 * 7;
        profile.statistics.best_territory_percent = 74.0 - index as f32 * 9.0;
    }
    world.resource_mut::<NextState<AppState>>().set(state);
}

fn configure_lobby(world: &mut World) {
    world.resource_mut::<Lobby>().players = vec![
        LobbyPlayer {
            device: InputDeviceId::Mouse,
            profile_id: None,
            display_name: "MOUSE ACE".into(),
            color_id: 0,
            pattern_id: 2,
            ready: true,
            connected: true,
        },
        LobbyPlayer {
            device: InputDeviceId::KeyboardPrimary,
            profile_id: None,
            display_name: "KEY KID".into(),
            color_id: 4,
            pattern_id: 7,
            ready: true,
            connected: true,
        },
    ];
    world.resource_mut::<Lobby>().total_competitors = 8;
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Lobby);
}

fn configure_match(world: &mut World) {
    let setup = MatchSetup {
        seed: 0x5eed_cafe,
        total_competitors: 8,
        humans: vec![
            HumanSetup {
                device: InputDeviceId::Mouse,
                profile_id: None,
                display_name: "MOUSE ACE".into(),
                color_id: 0,
                pattern_id: 2,
            },
            HumanSetup {
                device: InputDeviceId::KeyboardPrimary,
                profile_id: None,
                display_name: "KEY KID".into(),
                color_id: 4,
                pattern_id: 7,
            },
        ],
        replay_same_field: false,
    };
    *world.resource_mut::<MatchSetup>() = setup.clone();
    start_match(world, &setup);
    // Enter the real countdown state so its HUD and lifecycle hooks run, while
    // keeping automated captures fast and deterministic.
    world.resource_mut::<MatchSession>().phase = MatchPhase::Countdown;
    world.resource_mut::<MatchSession>().countdown_remaining = 0.05;
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Countdown);
}

fn configure_results(world: &mut World) {
    *world.resource_mut::<MatchResults>() = MatchResults {
        winner_name: "MOUSE ACE".into(),
        duration_seconds: 154.0,
        rows: vec![
            ResultRow {
                name: "MOUSE ACE".into(),
                color_id: 0,
                placement: 1,
                peak_percent: 100.0,
                kills: 6,
                deaths: 1,
                largest_capture_percent: 18.4,
                total_cells_captured: 12_480,
                longest_trail: 42.7,
            },
            ResultRow {
                name: "KEY KID".into(),
                color_id: 4,
                placement: 2,
                peak_percent: 32.8,
                kills: 3,
                deaths: 2,
                largest_capture_percent: 9.1,
                total_cells_captured: 5_220,
                longest_trail: 31.2,
            },
            ResultRow {
                name: "RUNE [NPC]".into(),
                color_id: 2,
                placement: 3,
                peak_percent: 24.6,
                kills: 2,
                deaths: 3,
                largest_capture_percent: 7.8,
                total_cells_captured: 4_810,
                longest_trail: 27.4,
            },
            ResultRow {
                name: "NOVA [NPC]".into(),
                color_id: 3,
                placement: 4,
                peak_percent: 18.9,
                kills: 2,
                deaths: 4,
                largest_capture_percent: 6.2,
                total_cells_captured: 3_990,
                longest_trail: 22.8,
            },
            ResultRow {
                name: "CRUMB [NPC]".into(),
                color_id: 6,
                placement: 5,
                peak_percent: 14.1,
                kills: 1,
                deaths: 4,
                largest_capture_percent: 5.0,
                total_cells_captured: 3_210,
                longest_trail: 19.6,
            },
            ResultRow {
                name: "FIZZ [NPC]".into(),
                color_id: 5,
                placement: 6,
                peak_percent: 10.8,
                kills: 1,
                deaths: 5,
                largest_capture_percent: 3.9,
                total_cells_captured: 2_760,
                longest_trail: 16.3,
            },
            ResultRow {
                name: "MOSS [NPC]".into(),
                color_id: 8,
                placement: 7,
                peak_percent: 8.4,
                kills: 0,
                deaths: 5,
                largest_capture_percent: 3.1,
                total_cells_captured: 2_180,
                longest_trail: 13.7,
            },
            ResultRow {
                name: "ZAP [NPC]".into(),
                color_id: 7,
                placement: 8,
                peak_percent: 5.3,
                kills: 0,
                deaths: 6,
                largest_capture_percent: 2.4,
                total_cells_captured: 1_540,
                longest_trail: 9.8,
            },
        ],
    };
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Results);
}

fn scenario_is_visible(world: &mut World) -> bool {
    let scenario = world.resource::<Harness>().scenario;
    let state = *world.resource::<State<AppState>>().get();
    if scenario == Scenario::Pause && state == AppState::Playing {
        let elapsed = world.resource::<MatchSession>().elapsed_seconds;
        if elapsed >= 3.0 {
            world
                .resource_mut::<NextState<AppState>>()
                .set(AppState::Paused);
        }
    }
    match scenario {
        Scenario::Home => state == AppState::Home,
        Scenario::Leaderboard => state == AppState::LocalLeaderboard,
        Scenario::Lobby => state == AppState::Lobby,
        Scenario::Match => state == AppState::Playing,
        Scenario::Pause => state == AppState::Paused,
        Scenario::Results => state == AppState::Results,
        Scenario::Settings => state == AppState::Settings,
    }
}
