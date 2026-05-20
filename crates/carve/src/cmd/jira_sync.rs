//! `carve jira-sync` — push the plan's per-ticket policy into JIRA.
//!
//! For each ticket the plan declares:
//!   1. Set story points (`TicketScope.story_points`) into the
//!      configured custom field. Carve never silently picks a value —
//!      if `story_points` is None, this ticket is skipped for points.
//!   2. Transition to `TicketScope.target_status` (or to a default if
//!      unset). Carve refuses to transition past
//!      `JiraConfig.max_auto_transition` — operators get a clear warning
//!      that they need to advance the ticket by hand.
//!   3. (Optional, off by default) post an ADF comment linking the PR.
//!
//! Story-point scale is policy-driven: the operator-supplied estimate is
//! in *days*, and the config's `points_per_day` factor maps it to the
//! number recorded in JIRA. Default: 1 point = 1 day.

use crate::{config, github, jira};
use anyhow::{Context, Result};
use carve_types::Plan;
use colored::Colorize;
use std::path::PathBuf;

pub struct Args {
    pub plan: PathBuf,
    pub no_points: bool,
    pub no_transition: bool,
}

pub fn run(args: Args) -> Result<()> {
    let cfg = config::load().context("load carve config")?;
    let yaml = std::fs::read_to_string(&args.plan)
        .with_context(|| format!("read {}", args.plan.display()))?;
    let plan = Plan::from_yaml(&yaml).context("parse plan.yaml")?;
    plan.check_basic_invariants()?;

    let client = jira::Client::from_env().context("create JIRA client")?;
    let policy = &cfg.jira;

    // Build a ticket → PR-url map. Each ticket has a stack node; each
    // node has a branch; the open/recently-closed PR for that branch is
    // what we link. Looking these up once at the top is cheaper than
    // re-querying inside the per-ticket loop.
    let pr_by_ticket: std::collections::HashMap<String, (u64, String)> = plan
        .stack
        .nodes
        .iter()
        .filter_map(|n| {
            let pr = github::pr_for_branch(&n.branch).ok().flatten()?;
            let url = format!(
                "https://github.com/{}/{}",
                pr_owner_repo().unwrap_or_else(|| "owner/repo".into()),
                format!("pull/{pr}"),
            );
            Some((n.ticket.to_string(), (pr, url)))
        })
        .collect();

    let mut points_set = 0usize;
    let mut transitions_done = 0usize;
    let mut skipped_past_max = 0usize;
    let mut comments_posted = 0usize;

    for t in &plan.tickets {
        let key = t.key.to_string();
        // 1. Story points.
        if !args.no_points {
            if let Some(pts) = t.story_points {
                client
                    .set_number_field(&key, &policy.story_points_field, pts)
                    .with_context(|| format!("set story points on {key}"))?;
                points_set += 1;
                println!(
                    "{} {} story_points = {pts}",
                    "✓".green(),
                    key.bold(),
                );
            } else {
                tracing::debug!(ticket = %key, "story_points unset in plan; skipping");
            }
        }

        // 2. Transition.
        if !args.no_transition {
            let target = match &t.target_status {
                Some(s) => s.clone(),
                None => continue, // operator didn't ask for a transition
            };
            if !policy.may_transition_to(&target) {
                println!(
                    "{} {} would-transition-to {:?} but carve policy caps at {:?} — leaving for human",
                    "!".yellow().bold(),
                    key.bold(),
                    target,
                    policy.max_auto_transition,
                );
                skipped_past_max += 1;
                continue;
            }
            // Resolve transition id: prefer config-pinned, else discover.
            let id = if let Some(id) = policy.transition_ids.get(&target) {
                id.to_string()
            } else {
                let avail = client.transitions(&key).context("fetch transitions")?;
                avail
                    .into_iter()
                    .find(|t| t.name.eq_ignore_ascii_case(&target))
                    .map(|t| t.id)
                    .ok_or_else(|| {
                        anyhow::anyhow!("ticket {key} has no transition named {target:?}")
                    })?
            };
            client
                .transition(&key, &id)
                .with_context(|| format!("transition {key} → {target}"))?;
            transitions_done += 1;
            println!(
                "{} {} transitioned to {} (id {})",
                "✓".green(),
                key.bold(),
                target.green(),
                id
            );
        }

        // 3. Optional ADF comment linking the PR.
        if policy.post_pr_link_comment {
            if let Some((pr, url)) = pr_by_ticket.get(&key) {
                let text = format!("Carve linked this ticket to PR #{pr}: {url}");
                client
                    .add_comment(&key, &text)
                    .with_context(|| format!("post PR-link comment on {key}"))?;
                comments_posted += 1;
                println!(
                    "{} {} comment posted (PR #{pr})",
                    "✓".green(),
                    key.bold()
                );
            } else {
                tracing::debug!(ticket = %key, "post_pr_link_comment enabled but no PR found for branch");
            }
        }
    }

    println!();
    println!(
        "{}: {} points set, {} transitions, {} comments, {} skipped past policy cap",
        "DONE".green().bold(),
        points_set,
        transitions_done,
        comments_posted,
        skipped_past_max,
    );
    if skipped_past_max > 0 {
        println!(
            "{}",
            "hint: set [jira] max_auto_transition in .carve.toml to allow further automation"
                .dimmed()
        );
    }
    Ok(())
}

/// Best-effort owner/repo extraction from `git config remote.origin.url`.
/// Falls back to `None` if it can't be determined; callers render a
/// placeholder URL in that case.
fn pr_owner_repo() -> Option<String> {
    let raw = crate::git::git(&["config", "--get", "remote.origin.url"]).ok()?;
    let raw = raw.trim();
    // SSH: git@github.com:owner/repo.git
    if let Some(rest) = raw.strip_prefix("git@github.com:") {
        return Some(rest.trim_end_matches(".git").to_string());
    }
    // HTTPS: https://github.com/owner/repo(.git)
    if let Some(rest) = raw.strip_prefix("https://github.com/") {
        return Some(rest.trim_end_matches(".git").trim_end_matches('/').to_string());
    }
    None
}
