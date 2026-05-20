//! `carve plan` — analyse the current branch and emit `plan.yaml`.
//!
//! Inputs:
//!   - JIRA epic key  (sub-tickets fetched via [`crate::jira`])
//!   - source branch  (defaults to current branch)
//!   - master branch  (defaults to `master`)
//!
//! Output:
//!   - `plan.yaml` containing the full [`carve_types::Plan`]
//!
//! Algorithm:
//!   1. Resolve `source` + `master` refs; compute merge-base.
//!   2. Walk commits in chronological order; fingerprint each.
//!   3. Fetch JIRA epic children; build a `TicketScope` per sub-task with
//!      *empty* `paths` — the operator fills these in.
//!   4. (When scopes are populated:) score each commit against scopes;
//!      single-scope commits get a `HighSingleScope` assignment; commits
//!      whose paths span multiple scopes become `CrossCuttingCommit`
//!      entries with a `ByPath` split proposal; unmatched commits get
//!      `Unassigned`.
//!   5. Build a [`StackTopology`] in `stack_order` order.
//!   6. Serialize to `plan.yaml`.
//!
//! After generation, the operator hand-edits `paths` on each `TicketScope`
//! and re-runs `carve plan --refresh` (TODO v0.2) to update assignments.
//! For now the first emission is the "shell" the operator fills.

use crate::{git, jira};
use anyhow::{Context, Result};
use carve_types::{
    commit::{CommitAssignment, Confidence},
    plan::{Plan, PlanMeta, SourceBranch},
    stack::{StackNode, StackTopology},
    ticket::{TicketKey, TicketScope},
};
use std::path::PathBuf;

pub struct Args {
    pub epic: String,
    pub branch: Option<String>,
    pub master_override: Option<String>,
    pub out: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    let branch = match args.branch {
        Some(b) => b,
        None => git::current_branch().context("could not determine current branch")?,
    };
    let tip = git::resolve(&branch)?;
    // Master ref: explicit override, else auto-detect from the remote's
    // HEAD. pleme-io repos are typically `origin/main`; akeyless-environments
    // is `origin/master`. Auto-detect via `git symbolic-ref` makes carve
    // work uniformly across both without operator-supplied flags.
    let master_ref = match args.master_override.as_deref() {
        Some(m) if git::resolve(m).is_ok() => m.to_string(),
        Some(m) => {
            let remote = format!("origin/{m}");
            if git::resolve(&remote).is_ok() {
                tracing::info!(used = %remote, "no local '{}' branch found; using '{}'", m, remote);
                remote
            } else {
                anyhow::bail!(
                    "--master {m} doesn't resolve as '{m}' or 'origin/{m}'",
                    m = m
                );
            }
        }
        None => {
            let detected = git::default_remote_branch().context("auto-detect master ref")?;
            tracing::info!(used = %detected, "auto-detected remote default branch");
            detected
        }
    };
    let master_sha = git::resolve(&master_ref)?;
    // `merge_base` is the *divergence point* — what carve records in the
    // plan so the operator can see when the branch forked. But for picking
    // the commits to carve, we walk `master_sha..tip` instead: that's the
    // set of commits reachable from the branch but NOT from master, which
    // excludes anything master picked up after divergence (automated
    // commits, other branches' merges, etc).
    let merge_base = git::merge_base(&master_sha, &tip)?;
    tracing::info!(branch = %branch, master = %master_ref, merge_base = %&merge_base[..12], master_tip = %&master_sha[..12], tip = %&tip[..12], "resolved range");

    let shas = git::commits_in_range(&master_sha, &tip)?;
    tracing::info!(commit_count = shas.len(), "walking commits");

    let mut commits = Vec::with_capacity(shas.len());
    for sha in &shas {
        let f = git::fingerprint(sha).with_context(|| format!("fingerprint {sha}"))?;
        commits.push(f);
    }

    // JIRA — fan epic into sub-tickets if env is configured. Otherwise
    // emit an empty `tickets:` list and let the operator hand-author.
    let tickets: Vec<TicketScope> = match jira::Client::from_env() {
        Ok(client) => match client.epic_children(&args.epic) {
            Ok(children) => children
                .into_iter()
                .enumerate()
                .map(|(idx, st)| TicketScope {
                    key: TicketKey::new(st.key.clone())
                        .unwrap_or_else(|_| panic!("invalid jira key {}", st.key)),
                    summary: st.summary,
                    paths: vec![],
                    exclude: vec![],
                    placeholder: false,
                    stack_order: (idx as u32) * 10,
                    story_points: None,
                    target_status: None,
                })
                .collect(),
            Err(e) => {
                tracing::warn!(err = %e, "could not fetch JIRA epic children — emitting empty tickets list");
                vec![]
            }
        },
        Err(e) => {
            tracing::warn!(err = %e, "ATLASSIAN_* env not configured — emitting empty tickets list");
            vec![]
        }
    };

    // First-emission assignments: every commit is `Unassigned`. Once the
    // operator populates ticket scopes, `carve plan --refresh` will
    // re-score; for now we just record the shells so nothing is dropped.
    // If JIRA wasn't reachable and there are no tickets, we skip emitting
    // assignments altogether — they reference ticket keys, so they can
    // only exist once tickets do. The operator will see the empty list
    // and know to either configure ATLASSIAN_* env or hand-author tickets.
    let assignments: Vec<CommitAssignment> = if tickets.is_empty() {
        Vec::new()
    } else {
        let first = tickets[0].key.clone();
        commits
            .iter()
            .map(|c| CommitAssignment {
                sha: c.sha.clone(),
                ticket: first.clone(),
                confidence: Confidence::Unassigned,
                paths_subset: None,
                rationale: Some("first plan emission — operator must populate scopes".into()),
            })
            .collect()
    };

    let stack = StackTopology {
        root: master_ref.clone(),
        nodes: tickets
            .iter()
            .enumerate()
            .map(|(idx, t)| {
                let base = if idx == 0 {
                    master_ref.clone()
                } else {
                    derive_branch_name(&tickets[idx - 1])
                };
                StackNode {
                    ticket: t.key.clone(),
                    branch: derive_branch_name(t),
                    base,
                    commits: vec![],
                    placeholder: t.placeholder,
                    pr_title: None,
                    pr_body: None,
                    draft: t.placeholder,
                }
            })
            .collect(),
    };

    let plan = Plan {
        meta: PlanMeta {
            carve_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            jira_epic: args.epic.clone(),
            operator: git_user_email().unwrap_or_else(|_| "unknown".into()),
        },
        source: SourceBranch {
            name: branch,
            master_branch: master_ref,
            tip,
            merge_base,
        },
        tickets,
        commits,
        assignments,
        cross_cutting: vec![],
        stack,
    };

    plan.check_basic_invariants()
        .context("generated plan failed basic invariants")?;

    let yaml = plan.to_yaml()?;
    std::fs::write(&args.out, yaml).with_context(|| format!("write {}", args.out.display()))?;
    println!("plan written: {}", args.out.display());
    if plan.tickets.is_empty() {
        println!("note: tickets list is empty — either configure ATLASSIAN_* env, or hand-author tickets in plan.yaml before `carve verify`.");
    } else {
        println!(
            "next: populate `paths` globs on each of the {} tickets, then run `carve verify`.",
            plan.tickets.len()
        );
    }
    Ok(())
}

fn derive_branch_name(t: &TicketScope) -> String {
    // Operator can hand-edit branch names in the plan; this is just a
    // sensible default. Convention: `<TICKET>-<short-slug-from-summary>`.
    let slug: String = t
        .summary
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("-");
    format!("{}-{}", t.key, slug)
}

fn git_user_email() -> Result<String> {
    Ok(git::git(&["config", "--get", "user.email"])?
        .trim()
        .to_string())
}
