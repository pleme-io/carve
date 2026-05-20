//! `carve execute` — apply a plan.
//!
//! TODO (v0.2): implement. Will:
//!   1. Read plan.yaml + verify invariants
//!   2. Create the BLAKE3-attested backup tag (cordel pattern)
//!   3. Create branches in stack_order, off origin/<master>
//!   4. For each commit: cherry-pick or apply split halves via
//!      `git show <sha> -- <paths> | git apply`
//!   5. Tree-hash gate: stack-top tree must equal source-branch tree
//!   6. Push branches (--set-upstream)
//!   7. `gh pr create` for each node, chaining --base
//!   8. Emit a final status JSON the operator can `jq` into other tools

use anyhow::Result;
use std::path::PathBuf;

pub struct Args {
    pub plan: PathBuf,
    pub no_push: bool,
    pub force: bool,
}

pub fn run(args: Args) -> Result<()> {
    let _ = args;
    anyhow::bail!("carve execute: not yet implemented (v0.2 target)");
}
