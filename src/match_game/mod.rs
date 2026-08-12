mod capture_systems;
mod lifecycle;
mod model;
mod npc_systems;
mod respawn;
mod systems;

pub use lifecycle::start_match;
pub use model::*;
pub use systems::MatchPlugin;
