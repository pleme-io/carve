//! `carve jira-sync` — sync story points + status transitions per the plan.
//!
//! TODO (v0.3): implement.

use anyhow::Result;
use std::path::PathBuf;

pub struct Args {
    pub plan: PathBuf,
    pub no_points: bool,
    pub no_transition: bool,
}

pub fn run(args: Args) -> Result<()> {
    let _ = args;
    anyhow::bail!("carve jira-sync: not yet implemented (v0.3 target)");
}
