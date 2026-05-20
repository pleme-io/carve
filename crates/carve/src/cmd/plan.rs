//! `carve plan` — analyse the current branch and emit `plan.yaml`.
//!
//! Two modes:
//!
//! - **Fresh emission** (`--epic <KEY>`): walk the branch, fetch JIRA
//!   sub-tickets, write a `plan.yaml` skeleton. Path-globs on each ticket
//!   are empty — operator fills them in.
//!
//! - **Refresh** (`--refresh`): re-read an existing `plan.yaml`, re-score
//!   every commit against the ticket scopes' current path-globs, and
//!   write the updated assignments + cross-cutting entries back. Any
//!   assignment marked `OperatorPinned` is preserved; any cross-cutting
//!   decision the operator has already made is preserved.

use crate::{git, jira};
use anyhow::{anyhow, Context, Result};
use carve_types::{
    commit::{CommitAssignment, CommitSha, Confidence},
    cross_cutting::{CrossCuttingCommit, MatchedScope, SplitDecision, SplitHalf},
    plan::{Plan, PlanMeta, SourceBranch},
    stack::{StackNode, StackTopology},
    ticket::{TicketKey, TicketScope},
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::PathBuf;

pub struct Args {
    pub epic: Option<String>,
    pub branch: Option<String>,
    pub master_override: Option<String>,
    pub out: PathBuf,
    pub refresh: bool,
}

pub fn run(args: Args) -> Result<()> {
    if args.refresh {
        return run_refresh(&args);
    }
    let epic = args
        .epic
        .as_ref()
        .ok_or_else(|| anyhow!("--epic is required for fresh emission (use --refresh to re-score an existing plan)"))?;
    run_fresh(&args, epic)
}

fn run_fresh(args: &Args, epic: &str) -> Result<()> {
    let branch = match &args.branch {
        Some(b) => b.clone(),
        None => git::current_branch().context("could not determine current branch")?,
    };
    let tip = git::resolve(&branch)?;
    let master_ref = resolve_master_ref(args.master_override.as_deref())?;
    let master_sha = git::resolve(&master_ref)?;
    let merge_base = git::merge_base(&master_sha, &tip)?;
    tracing::info!(branch = %branch, master = %master_ref, merge_base = %&merge_base[..12], master_tip = %&master_sha[..12], tip = %&tip[..12], "resolved range");

    let shas = git::commits_in_range(&master_sha, &tip)?;
    tracing::info!(commit_count = shas.len(), "walking commits");

    let mut commits = Vec::with_capacity(shas.len());
    for sha in &shas {
        let f = git::fingerprint(sha).with_context(|| format!("fingerprint {sha}"))?;
        commits.push(f);
    }

    let tickets: Vec<TicketScope> = match jira::Client::from_env() {
        Ok(client) => match client.epic_children(epic) {
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

    let stack = build_initial_stack(&tickets, &master_ref);
    let (assignments, cross_cutting) = score_assignments(&commits, &tickets)?;

    let plan = Plan {
        meta: PlanMeta {
            carve_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: now_rfc3339(),
            jira_epic: epic.to_string(),
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
        cross_cutting,
        stack,
    };

    plan.check_basic_invariants()
        .context("generated plan failed basic invariants")?;

    let yaml = plan.to_yaml()?;
    std::fs::write(&args.out, yaml).with_context(|| format!("write {}", args.out.display()))?;
    println!("plan written: {}", args.out.display());
    print_post_emission_hint(&plan);
    Ok(())
}

fn run_refresh(args: &Args) -> Result<()> {
    let yaml = std::fs::read_to_string(&args.out)
        .with_context(|| format!("read existing plan {}", args.out.display()))?;
    let mut plan = Plan::from_yaml(&yaml).context("parse plan.yaml")?;

    // Preserve operator-pinned assignments by SHA.
    let pinned: std::collections::HashMap<String, CommitAssignment> = plan
        .assignments
        .iter()
        .filter(|a| matches!(a.confidence, Confidence::OperatorPinned))
        .map(|a| (a.sha.to_string(), a.clone()))
        .collect();
    // Preserve operator-set cross-cutting decisions: keyed by SHA.
    let prev_decisions: std::collections::HashMap<String, CrossCuttingCommit> = plan
        .cross_cutting
        .iter()
        .cloned()
        .map(|cc| (cc.sha.to_string(), cc))
        .collect();

    let (mut fresh_assignments, mut fresh_cc) =
        score_assignments(&plan.commits, &plan.tickets)?;

    // Re-overlay pinned ones — they trump glob scoring.
    let pinned_keys: std::collections::HashSet<_> = pinned.keys().cloned().collect();
    fresh_assignments.retain(|a| !pinned_keys.contains(a.sha.as_str()));
    for a in pinned.values() {
        fresh_assignments.push(a.clone());
    }
    // Preserve operator decisions on cross-cutting commits whose paths
    // are unchanged (i.e. still flagged as cross-cutting after re-score).
    for cc in &mut fresh_cc {
        if let Some(prev) = prev_decisions.get(cc.sha.as_str()) {
            cc.decision = prev.decision.clone();
        }
    }

    plan.assignments = fresh_assignments;
    plan.cross_cutting = fresh_cc;

    // Refresh stack node commit lists from the new assignments (so a
    // newly-scored commit appears on its node).
    update_stack_commit_lists(&mut plan)?;

    plan.meta.generated_at = now_rfc3339();
    plan.meta.carve_version = env!("CARGO_PKG_VERSION").to_string();

    plan.check_basic_invariants()?;
    let yaml = plan.to_yaml()?;
    std::fs::write(&args.out, yaml)?;
    println!("plan refreshed: {}", args.out.display());
    print_post_emission_hint(&plan);
    Ok(())
}

fn resolve_master_ref(override_: Option<&str>) -> Result<String> {
    match override_ {
        Some(m) if git::resolve(m).is_ok() => Ok(m.to_string()),
        Some(m) => {
            let remote = format!("origin/{m}");
            if git::resolve(&remote).is_ok() {
                tracing::info!(used = %remote, "no local '{}' branch found; using '{}'", m, remote);
                Ok(remote)
            } else {
                anyhow::bail!("--master {m} doesn't resolve as '{m}' or 'origin/{m}'", m = m)
            }
        }
        None => {
            let detected = git::default_remote_branch().context("auto-detect master ref")?;
            tracing::info!(used = %detected, "auto-detected remote default branch");
            Ok(detected)
        }
    }
}

/// Path-glob scoring core.
///
/// Returns assignments + cross-cutting entries. The algorithm:
///
/// 1. Build a per-ticket [`GlobSet`] from `scope.paths` minus `scope.exclude`.
/// 2. For each commit, classify which tickets' globsets match at least
///    one of its paths.
/// 3. Zero matches  → `Unassigned`.
///    One match     → `HighSingleScope` assignment to that ticket.
///    Many matches  → `CrossCuttingCommit` with `ByPath` proposal,
///                    plus one `LowCrossCutting` assignment per matched
///                    ticket (carrying the per-ticket path subset).
pub(crate) fn score_assignments(
    commits: &[carve_types::CommitFingerprint],
    tickets: &[TicketScope],
) -> Result<(Vec<CommitAssignment>, Vec<CrossCuttingCommit>)> {
    let scope_sets: Vec<(TicketKey, GlobSet, GlobSet)> = tickets
        .iter()
        .map(|t| {
            let include = build_globset(&t.paths)?;
            let exclude = build_globset(&t.exclude)?;
            Ok::<_, anyhow::Error>((t.key.clone(), include, exclude))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut assignments = Vec::new();
    let mut cross_cutting = Vec::new();

    for c in commits {
        // Tickets that match this commit, plus the per-ticket path subset.
        let mut matches: Vec<(TicketKey, Vec<String>)> = Vec::new();
        for (key, inc, exc) in &scope_sets {
            // Empty include => the ticket has no scope declared yet; skip.
            if inc.is_empty() {
                continue;
            }
            let claimed: Vec<String> = c
                .paths
                .iter()
                .filter(|p| inc.is_match(p) && !exc.is_match(p))
                .cloned()
                .collect();
            if !claimed.is_empty() {
                matches.push((key.clone(), claimed));
            }
        }

        match matches.len() {
            0 => assignments.push(CommitAssignment {
                sha: c.sha.clone(),
                ticket: tickets
                    .first()
                    .map(|t| t.key.clone())
                    .unwrap_or_else(|| {
                        // No tickets at all — assignment can't reference
                        // a real ticket, so score_assignments only
                        // produces this path in tests. Synthesise a
                        // placeholder key that parses.
                        TicketKey::new("UNK-0").unwrap()
                    }),
                confidence: Confidence::Unassigned,
                paths_subset: None,
                rationale: Some("no ticket scope matches any path in this commit".into()),
            }),
            1 => {
                let (ticket, _paths) = matches.into_iter().next().unwrap();
                assignments.push(CommitAssignment {
                    sha: c.sha.clone(),
                    ticket,
                    confidence: Confidence::HighSingleScope,
                    paths_subset: None,
                    rationale: None,
                });
            }
            _ => {
                // Cross-cutting: emit one assignment per claimed ticket
                // and a CrossCuttingCommit with a ByPath proposal.
                let matched_scopes: Vec<MatchedScope> = matches
                    .iter()
                    .map(|(t, p)| MatchedScope {
                        ticket: t.clone(),
                        paths: p.clone(),
                    })
                    .collect();
                let halves: Vec<SplitHalf> = matches
                    .iter()
                    .map(|(t, p)| SplitHalf {
                        ticket: t.clone(),
                        paths: p.clone(),
                        subject_override: None,
                    })
                    .collect();
                for (ticket, paths) in &matches {
                    assignments.push(CommitAssignment {
                        sha: c.sha.clone(),
                        ticket: ticket.clone(),
                        confidence: Confidence::LowCrossCutting,
                        paths_subset: Some(paths.clone()),
                        rationale: Some(format!(
                            "{} of {} cross-cutting halves — review split before execute",
                            paths.len(),
                            c.paths.len()
                        )),
                    });
                }
                cross_cutting.push(CrossCuttingCommit {
                    sha: c.sha.clone(),
                    subject: c.subject.clone(),
                    matched_scopes,
                    decision: SplitDecision::ByPath { halves },
                });
            }
        }
    }

    Ok((assignments, cross_cutting))
}

fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(
            Glob::new(p).with_context(|| format!("invalid glob pattern: {p}"))?,
        );
    }
    b.build().map_err(|e| anyhow!(e))
}

fn build_initial_stack(tickets: &[TicketScope], master_ref: &str) -> StackTopology {
    StackTopology {
        root: master_ref.to_string(),
        nodes: tickets
            .iter()
            .enumerate()
            .map(|(idx, t)| {
                let base = if idx == 0 {
                    master_ref.to_string()
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
    }
}

fn update_stack_commit_lists(plan: &mut Plan) -> Result<()> {
    // For each node, collect the SHAs of commits whose (highest-priority)
    // assignment is to this node's ticket. Order = chronological (commits
    // are stored in chronological order).
    let mut by_ticket: std::collections::BTreeMap<String, Vec<CommitSha>> =
        std::collections::BTreeMap::new();
    for c in &plan.commits {
        let mut claimed_by: Vec<&TicketKey> = plan
            .assignments
            .iter()
            .filter(|a| a.sha == c.sha)
            .map(|a| &a.ticket)
            .collect();
        // Dedup by ticket key.
        claimed_by.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        claimed_by.dedup_by(|a, b| a.as_str() == b.as_str());
        for t in claimed_by {
            by_ticket
                .entry(t.to_string())
                .or_default()
                .push(c.sha.clone());
        }
    }
    for node in &mut plan.stack.nodes {
        node.commits = by_ticket
            .get(node.ticket.as_str())
            .cloned()
            .unwrap_or_default();
    }
    Ok(())
}

fn print_post_emission_hint(plan: &Plan) {
    if plan.tickets.is_empty() {
        println!("note: tickets list is empty — configure ATLASSIAN_* env, or hand-author tickets in plan.yaml.");
        return;
    }
    let empty_scope_count = plan.tickets.iter().filter(|t| t.paths.is_empty() && !t.placeholder).count();
    if empty_scope_count > 0 {
        println!(
            "note: {empty_scope_count} ticket(s) have no `paths` globs declared. Populate them, then `carve plan --refresh` to re-score."
        );
    }
    let unassigned = plan
        .assignments
        .iter()
        .filter(|a| matches!(a.confidence, Confidence::Unassigned))
        .count();
    let cross = plan.cross_cutting.len();
    if unassigned > 0 || cross > 0 {
        println!(
            "scoring: {} commit(s) Unassigned, {} cross-cutting — run `carve verify` for details.",
            unassigned, cross
        );
    } else {
        println!("scoring: all commits cleanly assigned. `carve verify` next, then `carve execute`.");
    }
}

fn derive_branch_name(t: &TicketScope) -> String {
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

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use carve_types::CommitFingerprint;

    fn fp(sha: &str, paths: &[&str]) -> CommitFingerprint {
        CommitFingerprint {
            sha: CommitSha::new(sha).unwrap(),
            subject: format!("test commit {sha}"),
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
            author: "test@x.io".into(),
            author_date: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn scope(key: &str, paths: &[&str]) -> TicketScope {
        TicketScope {
            key: TicketKey::new(key).unwrap(),
            summary: "test".into(),
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
            exclude: vec![],
            placeholder: false,
            stack_order: 0,
            story_points: None,
            target_status: None,
        }
    }

    #[test]
    fn single_scope_match_is_high_confidence() {
        let commits = vec![fp("aaaaaaa", &["backend/api.rs"])];
        let tickets = vec![scope("PROJ-1", &["backend/**"]), scope("PROJ-2", &["frontend/**"])];
        let (asn, cc) = score_assignments(&commits, &tickets).unwrap();
        assert_eq!(asn.len(), 1);
        assert_eq!(asn[0].ticket.as_str(), "PROJ-1");
        assert!(matches!(asn[0].confidence, Confidence::HighSingleScope));
        assert!(cc.is_empty());
    }

    #[test]
    fn cross_cutting_emits_split_proposal() {
        let commits = vec![fp("bbbbbbb", &["backend/api.rs", "frontend/app.tsx"])];
        let tickets = vec![scope("PROJ-1", &["backend/**"]), scope("PROJ-2", &["frontend/**"])];
        let (asn, cc) = score_assignments(&commits, &tickets).unwrap();
        assert_eq!(cc.len(), 1);
        assert_eq!(cc[0].matched_scopes.len(), 2);
        if let SplitDecision::ByPath { halves } = &cc[0].decision {
            assert_eq!(halves.len(), 2);
            // Each half claims exactly its scope's path.
            let h_proj1 = halves.iter().find(|h| h.ticket.as_str() == "PROJ-1").unwrap();
            assert_eq!(h_proj1.paths, vec!["backend/api.rs"]);
            let h_proj2 = halves.iter().find(|h| h.ticket.as_str() == "PROJ-2").unwrap();
            assert_eq!(h_proj2.paths, vec!["frontend/app.tsx"]);
        } else {
            panic!("expected ByPath decision");
        }
        // Two assignments emitted (one per claiming ticket).
        assert_eq!(asn.len(), 2);
        assert!(asn.iter().all(|a| matches!(a.confidence, Confidence::LowCrossCutting)));
    }

    #[test]
    fn no_scope_match_is_unassigned() {
        let commits = vec![fp("ccccccc", &["unrelated/thing"])];
        let tickets = vec![scope("PROJ-1", &["backend/**"])];
        let (asn, _cc) = score_assignments(&commits, &tickets).unwrap();
        assert_eq!(asn.len(), 1);
        assert!(matches!(asn[0].confidence, Confidence::Unassigned));
    }

    #[test]
    fn exclude_overrides_include() {
        let commits = vec![fp("ddddddd", &["backend/vendor/lib.rs"])];
        let tickets = vec![{
            let mut s = scope("PROJ-1", &["backend/**"]);
            s.exclude = vec!["backend/vendor/**".into()];
            s
        }];
        let (asn, _cc) = score_assignments(&commits, &tickets).unwrap();
        assert!(matches!(asn[0].confidence, Confidence::Unassigned));
    }
}
