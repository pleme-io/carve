//! `carve diagram` — regenerate the embedded stack diagram in each PR body.
//!
//! TODO (v0.5): implement. Uses an idempotent fence:
//!   <!-- carve:stack-diagram begin -->
//!   ...auto-regenerated content...
//!   <!-- carve:stack-diagram end -->

use anyhow::Result;
use std::path::PathBuf;

pub fn run(plan: PathBuf) -> Result<()> {
    let _ = plan;
    anyhow::bail!("carve diagram: not yet implemented (v0.5 target)");
}
