//! Deterministic behavior-laboratory fixtures for the authoritative simulation.
//!
//! The lab is an assembly of small modules: its public artifact model is kept
//! separate from fixture construction, recording, acceptance, and replay IO.

mod acceptance;
mod fixtures;
mod io;
mod model;
mod record;
mod replay;
mod runner;

pub use io::{read_artifact, write_artifact, write_svg};
pub use model::{
    EncounterFixture, LAB_FORMAT_VERSION, LAB_SETUP_VERSION, LabAcceptance, LabAcceptanceCheck,
    LabArtifact, LabDecisionSample, LabError, LabManeuverStats, LabReplay, LabReport, LabTrace,
    LabVariant, MAX_TICKS, MAX_TRACE_RECORDS, MAX_TRAJECTORY_POINTS, PersonalityVariant,
    RecordedEvent, ReplayTick,
};
pub use replay::verify_replay;
pub use runner::LabRunner;
