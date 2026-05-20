//! `carve gate` — CI hook refusing out-of-order merges.
//!
//! TODO (v0.5): implement.

use anyhow::Result;
use std::path::PathBuf;

pub struct Args {
    pub pr: u64,
    pub plan: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    let _ = args;
    anyhow::bail!("carve gate: not yet implemented (v0.5 target)");
}
