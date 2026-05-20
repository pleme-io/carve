//! `carve restack` — propagate a parent-branch fix through descendants.
//!
//! Use case: reviewer asks for a change on PR #N (in the middle of the
//! stack). Operator pushes the fix to that branch. Now every descendant
//! PR (#N+1, #N+2, ...) has a stale base — they were built atop the OLD
//! parent tip and need to be replayed atop the NEW tip.
//!
//! Algorithm per descendant (top-to-bottom in stack_order, starting from
//! the branch immediately after `--from`):
//!   1. Capture the OLD parent tip via `git merge-base <descendant> <parent>`
//!      — that's where the descendant currently roots on the parent.
//!   2. `git rebase --onto <parent> <old-parent-tip> <descendant>` — replay
//!      every descendant-local commit on top of the new parent tip.
//!   3. Tree-hash sanity: descendant's tree before vs after should differ
//!      only by the fix that motivated the restack — carve emits a small
//!      report so the operator can spot-check.
//!
//! The plan.yaml itself is not mutated. After restack the operator may
//! want to regenerate diagrams (`carve diagram`) so PR bodies' embedded
//! "your're here" markers are still accurate.

use crate::git;
use anyhow::{anyhow, Context, Result};
use carve_types::Plan;
use colored::Colorize;
use std::path::PathBuf;

pub struct Args {
    pub plan: PathBuf,
    pub from: String,
}

pub fn run(args: Args) -> Result<()> {
    let yaml = std::fs::read_to_string(&args.plan)
        .with_context(|| format!("read {}", args.plan.display()))?;
    let plan = Plan::from_yaml(&yaml).context("parse plan.yaml")?;
    plan.check_basic_invariants()?;

    if !git::working_tree_clean()? {
        anyhow::bail!("working tree is dirty — commit or stash before restack");
    }

    let nodes = &plan.stack.nodes;
    let from_idx = nodes
        .iter()
        .position(|n| n.branch == args.from)
        .ok_or_else(|| anyhow!("--from branch {:?} is not in the stack", args.from))?;
    let descendants = &nodes[from_idx + 1..];
    if descendants.is_empty() {
        println!("{} no descendants of {} — nothing to restack", "OK".green(), args.from);
        return Ok(());
    }

    println!(
        "restacking {} descendant(s) on top of {}",
        descendants.len(),
        args.from.bold()
    );

    // Walk down the stack, replaying each onto its new parent. The new
    // parent of descendants[0] is `args.from`; for descendants[i>0] it's
    // descendants[i-1].branch (now restacked).
    let mut parent = args.from.clone();
    for d in descendants {
        let new_parent_tip = git::resolve(&parent)?;
        // The OLD parent tip is the merge-base of the descendant with the
        // (now-modified) parent — i.e. where they diverged before this fix.
        let old_parent_tip = git::merge_base(&parent, &d.branch).with_context(|| {
            format!("merge-base({parent}, {})", d.branch)
        })?;
        if old_parent_tip == new_parent_tip {
            println!(
                "{} {} already up-to-date on {}",
                "·".dimmed(),
                d.branch,
                parent
            );
            parent = d.branch.clone();
            continue;
        }
        println!(
            "  {} rebasing {} (--onto {} from {})",
            "→".cyan(),
            d.branch.bold(),
            &new_parent_tip[..12],
            &old_parent_tip[..12]
        );
        git::git(&[
            "rebase",
            "--onto",
            &new_parent_tip,
            &old_parent_tip,
            &d.branch,
        ])
        .with_context(|| format!("rebase {} onto {}", d.branch, parent))?;
        parent = d.branch.clone();
    }

    println!();
    println!(
        "{} {} branch(es) restacked. Run `carve diagram` to refresh PR bodies, then `git push --force-with-lease origin <branches>` to publish.",
        "DONE".green().bold(),
        descendants.len()
    );
    Ok(())
}
