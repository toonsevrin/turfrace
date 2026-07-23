//! Turfrace game library.
//!
//! The executable is intentionally only composition. Authoritative gameplay,
//! platform integration, and presentation remain independently testable plugins.

pub mod app_state;
pub mod audio;
pub mod board;
pub mod camera;
pub mod capture;
pub mod combat;
pub mod config;
pub mod effects;
pub mod geometry;
pub mod ids;
pub mod input;
pub mod lobby;
pub mod match_game;
pub mod movement;
pub mod npc;
pub mod palette;
pub mod presentation;
pub mod profiles;
pub mod render;
pub mod territory_map;
pub mod trail;
pub mod ui;
pub mod web;
