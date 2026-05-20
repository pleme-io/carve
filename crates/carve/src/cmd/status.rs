//! `carve status` — stack health snapshot.
//!
//! Reports, per stack node:
//!   - The PR number (if any), state, and draft flag.
//!   - Local-branch presence + whether it matches origin.
//!   - Base branch's PR state — exposes parent-not-yet-merged conditions
//!     even before `carve gate` blocks them in CI.
//!
//! Pure read-only — never mutates anything.

use crate::{git, github};
use anyhow::{Context, Result};
use carve_types::Plan;
use colored::Colorize;
use std::path::PathBuf;

pub fn run(plan_path: PathBuf) -> Result<()> {
    let yaml = std::fs::read_to_string(&plan_path)
        .with_context(|| format!("read {}", plan_path.display()))?;
    let plan = Plan::from_yaml(&yaml).context("parse plan.yaml")?;
    plan.check_basic_invariants()?;

    println!(
        "stack of {} node(s) for epic {}",
        plan.stack.nodes.len(),
        plan.meta.jira_epic.bold()
    );
    println!("rooted on: {}", plan.stack.root);
    println!();

    for (idx, n) in plan.stack.nodes.iter().enumerate() {
        let pr_info = match github::pr_for_branch(&n.branch)? {
            None => "(no PR)".dimmed().to_string(),
            Some(pr) => {
                let state = github::pr_state(pr).unwrap_or_else(|_| "?".into());
                let color = match state.as_str() {
                    "MERGED" => "MERGED".green(),
                    "CLOSED" => "CLOSED".red(),
                    "OPEN" => "OPEN".cyan(),
                    other => other.normal(),
                };
                format!("#{pr} [{color}]")
            }
        };
        let local_state = if git::branch_exists(&n.branch) {
            // Compare local tip vs origin/<branch>.
            let remote_ref = format!("origin/{}", n.branch);
            match (git::resolve(&n.branch), git::resolve(&remote_ref)) {
                (Ok(local), Ok(remote)) if local == remote => "in-sync".green().to_string(),
                (Ok(_), Ok(_)) => "DRIFT (local ≠ origin)".yellow().bold().to_string(),
                (Ok(_), Err(_)) => "local-only".dimmed().to_string(),
                _ => "?".dimmed().to_string(),
            }
        } else {
            "(no local branch)".dimmed().to_string()
        };
        let marker = if n.placeholder { "□" } else { "■" };
        println!(
            "{idx:>2}. {marker} {ticket}  {branch}",
            ticket = n.ticket.to_string().bold(),
            branch = n.branch.dimmed(),
        );
        println!("       pr:    {pr_info}");
        println!("       local: {local_state}");
        println!("       base:  {}", n.base);
        println!();
    }
    Ok(())
}
