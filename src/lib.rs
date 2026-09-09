//! Turfrace game library.
//!
//! The executable is intentionally only composition. Authoritative gameplay,
//! platform integration, and presentation remain independently testable plugins.

#[cfg(feature = "shell")]
pub mod app_state;
#[cfg(feature = "shell")]
pub mod audio;
pub mod board;
#[cfg(feature = "shell")]
pub mod camera;
pub mod capture;
pub mod combat;
pub mod config;
#[cfg(feature = "shell")]
pub mod effects;
pub mod geometry;
pub mod ids;
#[cfg(feature = "shell")]
pub mod input;
#[cfg(feature = "shell")]
pub mod lobby;
pub mod match_game;
pub mod movement;
pub mod npc;
#[cfg(feature = "shell")]
pub mod palette;
#[cfg(feature = "shell")]
pub mod presentation;
#[cfg(feature = "shell")]
pub mod profiles;
#[cfg(feature = "shell")]
pub mod render;
pub mod territory_map;
pub mod trail;
#[cfg(feature = "shell")]
pub mod ui;
#[cfg(feature = "shell")]
pub mod web;
