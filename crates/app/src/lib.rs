//! Shared native UI bindings for the player and its integration tests.
//!
//! Compile Slint's generated Rust once instead of separately in every test
//! executable. The integration tests exercise these exact player bindings.
slint::include_modules!();

pub mod lyrics_effects;
