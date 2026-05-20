//! Subcommand implementations. Each subcommand lives in its own module
//! and exposes a single `run` entry point + an `Args` struct (if it has
//! parameters beyond a plan path).

pub mod diagram;
pub mod execute;
pub mod gate;
pub mod jira_sync;
pub mod plan;
pub mod restack;
pub mod status;
pub mod verify;
