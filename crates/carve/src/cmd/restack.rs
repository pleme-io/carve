//! `carve restack` — propagate review-feedback changes through descendants.
//!
//! TODO (v0.4): implement.

use anyhow::Result;
use std::path::PathBuf;

pub struct Args {
    pub plan: PathBuf,
    pub from: String,
}

pub fn run(args: Args) -> Result<()> {
    let _ = args;
    anyhow::bail!("carve restack: not yet implemented (v0.4 target)");
}
