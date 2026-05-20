//! `carve gate` — CI hook refusing out-of-order merges.
//!
//! Given a PR number and the plan, refuse the merge if any parent PR in
//! the stack is still open (i.e. not merged or closed-with-replacement).
//!
//! Designed to plug into a GitHub Actions `pull_request` workflow so a
//! reviewer who clicks Merge on PR #N while PR #N-1 is still pending
//! gets a clear failure rather than producing an out-of-order merge.

use crate::github;
use anyhow::{anyhow, Context, Result};
use carve_types::Plan;
use colored::Colorize;
use std::path::PathBuf;

pub struct Args {
    pub pr: u64,
    pub plan: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    let yaml = std::fs::read_to_string(&args.plan)
        .with_context(|| format!("read {}", args.plan.display()))?;
    let plan = Plan::from_yaml(&yaml).context("parse plan.yaml")?;
    plan.check_basic_invariants()?;

    // Find this PR's node by looking up the head branch among stack nodes.
    // The CI workflow passes --pr; we resolve back to a node via
    // gh pr view --json headRefName.
    let head_branch = pr_head_branch(args.pr)?;
    let idx = plan
        .stack
        .nodes
        .iter()
        .position(|n| n.branch == head_branch)
        .ok_or_else(|| {
            anyhow!(
                "PR #{} (head={}) is not in the stack defined by {}",
                args.pr,
                head_branch,
                args.plan.display()
            )
        })?;

    if idx == 0 {
        println!(
            "{} PR #{} is the stack base — no parent to gate on. OK",
            "✓".green(),
            args.pr
        );
        return Ok(());
    }

    let mut blocking = Vec::new();
    for parent in &plan.stack.nodes[..idx] {
        let Some(parent_pr) = github::pr_for_branch(&parent.branch)? else {
            tracing::warn!(branch = %parent.branch, "no PR found for parent branch; treating as not-yet-merged");
            blocking.push(format!("{} (no PR found)", parent.branch));
            continue;
        };
        let state = github::pr_state(parent_pr)?;
        if state != "MERGED" && state != "CLOSED" {
            blocking.push(format!("#{parent_pr} ({} state={state})", parent.branch));
        }
    }

    if blocking.is_empty() {
        println!(
            "{} PR #{} has all {} stack parent(s) merged/closed. OK to merge.",
            "✓".green().bold(),
            args.pr,
            idx
        );
        Ok(())
    } else {
        println!(
            "{} PR #{} has {} unmerged stack parent(s):",
            "BLOCK".red().bold(),
            args.pr,
            blocking.len()
        );
        for b in &blocking {
            println!("  - {b}");
        }
        anyhow::bail!("refusing out-of-order merge — resolve parents first")
    }
}

fn pr_head_branch(pr: u64) -> Result<String> {
    let out = std::process::Command::new("gh")
        .args(["pr", "view", &pr.to_string(), "--json", "headRefName", "--jq", ".headRefName"])
        .output()
        .context("spawn gh pr view")?;
    if !out.status.success() {
        anyhow::bail!(
            "gh pr view {} failed: {}",
            pr,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}
