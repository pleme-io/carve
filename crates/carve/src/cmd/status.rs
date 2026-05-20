//! `carve status` — stack health snapshot.
//!
//! TODO (v0.5): implement. Reports per-PR merge state, base-chain drift,
//! and JIRA status divergence vs the plan.

use anyhow::Result;
use std::path::PathBuf;

pub fn run(plan: PathBuf) -> Result<()> {
    let _ = plan;
    anyhow::bail!("carve status: not yet implemented (v0.5 target)");
}
