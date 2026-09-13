//! Headless, production-simulation NPC behavior laboratory.
//!
//! Examples:
//!   cargo run --no-default-features --example npc_lab -- --fixture return-race --ticks 1800 --trace target/npc-lab/return-race.json
//!   cargo run --no-default-features --example npc_lab -- replay target/npc-lab/return-race.json --verify

use std::{env, error::Error, path::PathBuf};

use turfrace::npc::{
    NpcDifficulty,
    lab::{
        EncounterFixture, LabRunner, LabVariant, PersonalityVariant, read_artifact, verify_replay,
        write_artifact, write_svg,
    },
};

#[derive(Debug)]
struct Options {
    fixture: EncounterFixture,
    field_seed: Option<u64>,
    npc_seed: Option<u64>,
    variant: LabVariant,
    ticks: u64,
    trace: Option<PathBuf>,
    svg: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|value| value == "replay") {
        args.remove(0);
        let path = args
            .first()
            .cloned()
            .ok_or("replay requires an artifact path")?;
        let verify = args.iter().skip(1).any(|value| value == "--verify");
        reject_unknown_replay_flags(&args[1..])?;
        let artifact = read_artifact(path)?;
        if verify {
            verify_replay(&artifact.replay)?;
            print_acceptance(&artifact.report.acceptance);
            println!(
                "replay-verified fixture={} ticks={} acceptance={}",
                artifact.replay.fixture, artifact.replay.ticks, artifact.report.acceptance.passed
            );
            if !artifact.report.acceptance.passed {
                return Err(format!(
                    "fixture {} failed acceptance; see artifact checks",
                    artifact.replay.fixture
                )
                .into());
            }
        } else {
            println!(
                "fixture={} ticks={} acceptance={} (replay not verified)",
                artifact.replay.fixture, artifact.replay.ticks, artifact.report.acceptance.passed
            );
            print_acceptance(&artifact.report.acceptance);
        }
        return Ok(());
    }

    let options = parse_options(&args)?;
    let (default_field, default_npc) = options.fixture.default_seeds();
    let runner = LabRunner::new(
        options.fixture,
        options.field_seed.unwrap_or(default_field),
        options.npc_seed.unwrap_or(default_npc),
        options.variant,
        options.ticks,
    )?;
    let artifact = runner.run()?;
    if let Some(path) = options.trace {
        write_artifact(path, &artifact)?;
    }
    if let Some(path) = options.svg {
        write_svg(path, &artifact.report)?;
    }
    println!(
        "fixture={} ticks={} captures={} deaths={} kills={} acceptance={}",
        artifact.report.fixture,
        artifact.report.stats.fixed_ticks,
        artifact.report.stats.captures,
        artifact.report.stats.deaths,
        artifact.report.stats.kills,
        artifact.report.acceptance.passed,
    );
    if artifact.report.fixture == EncounterFixture::BridgeCapture {
        println!("  bridge-capture is a topology diagnostic, not an NPC acceptance fixture");
    }
    print_acceptance(&artifact.report.acceptance);
    if !artifact.report.acceptance.passed {
        return Err(format!(
            "fixture {} failed acceptance; see artifact checks",
            artifact.report.fixture
        )
        .into());
    }
    Ok(())
}

fn print_acceptance(acceptance: &turfrace::npc::lab::LabAcceptance) {
    for check in &acceptance.checks {
        println!(
            "  acceptance {}={} observed={} requirement={}",
            check.name, check.passed, check.observed, check.requirement
        );
    }
}

fn parse_options(args: &[String]) -> Result<Options, Box<dyn Error>> {
    let mut fixture = EncounterFixture::ReturnRace;
    let mut field_seed = None;
    let mut npc_seed = None;
    let mut difficulty = NpcDifficulty::Normal;
    let mut personality = PersonalityVariant::Baseline;
    let mut ticks = 1_800;
    let mut trace = None;
    let mut svg = None;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = |index: &mut usize| -> Result<String, Box<dyn Error>> {
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| format!("{flag} requires a value").into())
        };
        match flag {
            "--fixture" => fixture = value(&mut index)?.parse()?,
            "--field-seed" => field_seed = Some(parse_seed(&value(&mut index)?)?),
            "--npc-seed" => npc_seed = Some(parse_seed(&value(&mut index)?)?),
            "--difficulty" => difficulty = parse_difficulty(&value(&mut index)?)?,
            "--personality" => personality = value(&mut index)?.parse()?,
            "--ticks" => ticks = value(&mut index)?.parse()?,
            "--trace" | "--record" => trace = Some(PathBuf::from(value(&mut index)?)),
            "--svg" => svg = Some(PathBuf::from(value(&mut index)?)),
            "--overlay" => {
                return Err("--overlay requires the shell visual-playtest integration; use --svg for headless output".into())
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unknown option {other:?}").into()),
        }
        index += 1;
    }
    Ok(Options {
        fixture,
        field_seed,
        npc_seed,
        variant: LabVariant {
            difficulty,
            personality,
            ..LabVariant::default()
        },
        ticks,
        trace,
        svg,
    })
}

fn parse_seed(raw: &str) -> Result<u64, Box<dyn Error>> {
    let explicit_hex = raw.starts_with("0x");
    let value = raw.strip_prefix("0x").unwrap_or(raw);
    let radix = if explicit_hex || !value.chars().all(|character| character.is_ascii_digit()) {
        16
    } else {
        10
    };
    u64::from_str_radix(value, radix)
        .map_err(|error| format!("invalid seed {raw:?}: {error}").into())
}

fn parse_difficulty(value: &str) -> Result<NpcDifficulty, Box<dyn Error>> {
    match value {
        "easy" => Ok(NpcDifficulty::Easy),
        "normal" => Ok(NpcDifficulty::Normal),
        "hard" => Ok(NpcDifficulty::Hard),
        _ => Err(format!("unknown difficulty {value:?}").into()),
    }
}

fn reject_unknown_replay_flags(flags: &[String]) -> Result<(), Box<dyn Error>> {
    for flag in flags {
        if flag != "--verify" {
            return Err(format!("unknown replay option {flag:?}").into());
        }
    }
    Ok(())
}

fn print_help() {
    println!(
        "npc_lab [--fixture NAME] [--field-seed SEED] [--npc-seed SEED] [--difficulty easy|normal|hard] [--personality baseline|builder|hunter|raider] [--ticks 1..7200] [--trace PATH] [--record PATH] [--svg PATH]"
    );
    println!("npc_lab replay PATH --verify");
}
