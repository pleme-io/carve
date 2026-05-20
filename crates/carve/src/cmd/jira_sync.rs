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

use crate::{config, jira};
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

    let mut points_set = 0usize;
    let mut transitions_done = 0usize;
    let mut skipped_past_max = 0usize;

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
    }

    println!();
    println!(
        "{}: {} points set, {} transitions, {} skipped past policy cap",
        "DONE".green().bold(),
        points_set,
        transitions_done,
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
