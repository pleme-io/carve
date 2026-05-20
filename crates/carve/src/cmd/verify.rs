//! `carve verify` — dry-run all invariants on a plan.yaml without mutating
//! anything. Surfaces:
//!   - Unassigned commits (operator must extend ticket scopes or hand-pin)
//!   - Cross-cutting commits whose split decisions are incomplete
//!   - Stack-order violations
//!   - Tickets with placeholder=true but commits assigned (illegal)

use anyhow::{Context, Result};
use carve_types::{Plan, commit::Confidence, cross_cutting::SplitDecision};
use colored::Colorize;
use std::path::PathBuf;

pub fn run(plan_path: PathBuf) -> Result<()> {
    let yaml =
        std::fs::read_to_string(&plan_path).with_context(|| format!("read {}", plan_path.display()))?;
    let plan = Plan::from_yaml(&yaml).context("parse plan.yaml")?;
    plan.check_basic_invariants()
        .context("plan failed basic invariants")?;

    let mut warnings = 0_usize;
    let mut errors = 0_usize;

    // 1. Unassigned commits.
    let unassigned = plan
        .assignments
        .iter()
        .filter(|a| matches!(a.confidence, Confidence::Unassigned))
        .count();
    if unassigned > 0 {
        println!(
            "{} {} commit(s) still Unassigned — populate ticket scope `paths` or hand-pin.",
            "warn:".yellow().bold(),
            unassigned
        );
        warnings += 1;
    }

    // 2. Cross-cutting decisions that need attention.
    let pending_drops = plan
        .cross_cutting
        .iter()
        .filter(|c| match &c.decision {
            SplitDecision::Drop { rationale } => rationale.trim().is_empty(),
            _ => false,
        })
        .count();
    if pending_drops > 0 {
        println!(
            "{} {} cross-cutting commit(s) marked Drop with no rationale.",
            "error:".red().bold(),
            pending_drops
        );
        errors += 1;
    }

    // 3. Placeholder tickets must not have assignments.
    let placeholder_keys: std::collections::HashSet<_> = plan
        .tickets
        .iter()
        .filter(|t| t.placeholder)
        .map(|t| t.key.as_str())
        .collect();
    let illegal_placeholder_assignments = plan
        .assignments
        .iter()
        .filter(|a| placeholder_keys.contains(a.ticket.as_str()))
        .count();
    if illegal_placeholder_assignments > 0 {
        println!(
            "{} {} commit(s) assigned to placeholder tickets — placeholders cannot carry code.",
            "error:".red().bold(),
            illegal_placeholder_assignments
        );
        errors += 1;
    }

    // 4. Summary.
    println!();
    println!(
        "{}: {} tickets, {} commits, {} cross-cutting, {} stack nodes",
        "plan".bold(),
        plan.tickets.len(),
        plan.commits.len(),
        plan.cross_cutting.len(),
        plan.stack.nodes.len()
    );
    if errors == 0 && warnings == 0 {
        println!("{}", "verify OK — plan is ready for `carve execute`.".green());
        Ok(())
    } else if errors == 0 {
        println!("{}", format!("verify ok with {warnings} warning(s).").yellow());
        Ok(())
    } else {
        anyhow::bail!("verify failed: {errors} error(s), {warnings} warning(s)")
    }
}
