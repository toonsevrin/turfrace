mod capture_systems;
mod headless;
mod interface;
mod lifecycle;
mod model;
mod npc_systems;
mod outcomes;
mod replay;
mod respawn;
mod rules;
#[cfg(feature = "shell")]
mod shell;
mod systems;

pub use headless::*;
pub use interface::*;
pub use lifecycle::start_simulation;
pub use model::*;
pub use replay::*;
pub use rules::MatchRules;
#[cfg(feature = "shell")]
pub use shell::{MatchPlugin, start_match};
pub use systems::SimulationPlugin;

#[cfg(test)]
#[path = "outcomes_tests.rs"]
mod outcomes_tests;
