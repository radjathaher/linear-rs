//! Core library for shared Linear integrations used by both CLI and TUI front-ends.

pub mod auth;
pub mod config;

/// Entry point used by early scaffolding binaries until real initialization exists.
pub fn init() -> anyhow::Result<()> {
    Ok(())
}
