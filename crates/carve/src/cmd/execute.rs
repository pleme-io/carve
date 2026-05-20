//! `carve execute` — apply a plan.
//!
//! Phases:
//!   1. Load plan + invariant pre-checks (working tree clean, no
//!      cross-cutting decisions still pending, etc.)
//!   2. Create the BLAKE3-attested backup tag pointing at the source
//!      branch tip — recovery anchor before any mutation.
//!   3. Build each stack node's branch in order:
//!       - placeholder nodes: branch off base + one `--allow-empty` commit
//!       - regular nodes:    branch off base, cherry-pick commits in
//!         chronological order, applying split halves where the commit
//!         has a CrossCuttingCommit entry.
//!   4. Tree-hash gate: the tree at the top of the stack MUST equal the
//!      source branch tip's tree. If not, carve refuses to push.
//!   5. (unless --no-push) push the backup tag + all stack branches via
//!      `--force-with-lease`, then `gh pr create --base <parent>` for
//!      each node with chained base refs and the rendered PR body.

use crate::{git, github};
use anyhow::{anyhow, Context, Result};
use carve_types::{
    attestation::{BackupTag, BlakeHash},
    commit::Confidence,
    cross_cutting::{CrossCuttingCommit, SplitDecision, SplitHalf},
    Plan,
};
use colored::Colorize;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct Args {
    pub plan: PathBuf,
    pub no_push: bool,
    pub force: bool,
}

pub fn run(args: Args) -> Result<()> {
    let yaml = std::fs::read_to_string(&args.plan)
        .with_context(|| format!("read {}", args.plan.display()))?;
    let plan = Plan::from_yaml(&yaml).context("parse plan.yaml")?;
    plan.check_basic_invariants()?;

    // Pre-flight: refuse if anything is still in operator-action territory.
    preflight(&plan)?;

    if !git::working_tree_clean()? {
        anyhow::bail!(
            "working tree is dirty — commit or stash before `carve execute` (refuses to operate on a dirty tree)"
        );
    }

    // 1. Backup tag.
    let backup = create_backup_tag(&plan, &yaml)?;
    println!(
        "{} backup tag {} → {}",
        "✓".green().bold(),
        backup.tag_name.bold(),
        &backup.original_tip[..12]
    );

    // Build a lookup: SHA → CrossCuttingCommit, for fast access when
    // walking each node's commit list.
    let cross_by_sha: BTreeMap<&str, &CrossCuttingCommit> = plan
        .cross_cutting
        .iter()
        .map(|c| (c.sha.as_str(), c))
        .collect();

    // 2. Build each branch.
    for node in &plan.stack.nodes {
        build_node(&plan, node, &cross_by_sha, args.force)?;
        println!(
            "{} built {} ({} commits)",
            "✓".green().bold(),
            node.branch.bold(),
            node.commits.len() + if node.placeholder { 1 } else { 0 }
        );
    }

    // 3. Tree-hash gate.
    let top_node = plan
        .stack
        .nodes
        .last()
        .ok_or_else(|| anyhow!("plan has no stack nodes — nothing to verify"))?;
    let want = git::tree_hash(&plan.source.tip)?;
    let got = git::tree_hash(&top_node.branch)?;
    if want != got {
        anyhow::bail!(
            "tree-hash gate FAILED: stack-top tree {} != source-branch tree {} — content drift detected, refusing to push",
            &got[..12],
            &want[..12]
        );
    }
    println!(
        "{} tree-hash gate PASSED: {} (stack top == source branch)",
        "✓".green().bold(),
        &want[..12].green()
    );

    if args.no_push {
        println!("{}", "skipping push (--no-push)".yellow());
        return Ok(());
    }

    // 4. Push backup tag + all branches.
    git::push_tag(&backup.tag_name).context("push backup tag")?;
    println!("{} pushed tag {}", "✓".green().bold(), backup.tag_name);

    let branch_refs: Vec<&str> = plan
        .stack
        .nodes
        .iter()
        .map(|n| n.branch.as_str())
        .collect();
    git::push_with_lease(&branch_refs).context("push stack branches")?;
    println!(
        "{} pushed {} branches with --force-with-lease",
        "✓".green().bold(),
        branch_refs.len()
    );

    // 5. Open PRs in stack order.
    for node in &plan.stack.nodes {
        let title = node
            .pr_title
            .clone()
            .unwrap_or_else(|| default_pr_title(&plan, node));
        let body = node
            .pr_body
            .clone()
            .unwrap_or_else(|| default_pr_body(&plan, node));
        let draft = node.draft || node.placeholder;
        let url = github::create_pr(&node.base, &node.branch, &title, &body, draft)
            .with_context(|| format!("create PR for {}", node.branch))?;
        println!(
            "{} PR {}: {}{}",
            "✓".green().bold(),
            url.trim().bold(),
            node.ticket,
            if draft { " [draft]".dimmed().to_string() } else { String::new() }
        );
    }

    println!();
    println!(
        "{} stack of {} PR(s) ready for review.",
        "DONE".green().bold(),
        plan.stack.nodes.len()
    );
    Ok(())
}

/// Refuse to execute a plan that has operator-action items pending.
fn preflight(plan: &Plan) -> Result<()> {
    // No assignment may still be Unassigned.
    if plan
        .assignments
        .iter()
        .any(|a| matches!(a.confidence, Confidence::Unassigned))
    {
        anyhow::bail!(
            "plan has unassigned commit(s) — run `carve verify` for details, then populate ticket scopes"
        );
    }
    // Cross-cutting Drop entries must have a rationale.
    for cc in &plan.cross_cutting {
        if let SplitDecision::Drop { rationale } = &cc.decision {
            if rationale.trim().is_empty() {
                anyhow::bail!(
                    "cross-cutting commit {} is marked Drop with no rationale — refuse to silently lose history",
                    cc.sha
                );
            }
        }
    }
    // Cross-cutting ByPath halves must cover all original paths exactly
    // once (no overlap, no drop).
    for cc in &plan.cross_cutting {
        if let SplitDecision::ByPath { halves } = &cc.decision {
            check_split_coverage(cc, halves)?;
        }
    }
    Ok(())
}

fn check_split_coverage(cc: &CrossCuttingCommit, halves: &[SplitHalf]) -> Result<()> {
    let all_paths: std::collections::HashSet<String> = cc
        .matched_scopes
        .iter()
        .flat_map(|m| m.paths.iter().cloned())
        .collect();
    let mut claimed: std::collections::HashSet<String> = std::collections::HashSet::new();
    for h in halves {
        for p in &h.paths {
            if !claimed.insert(p.clone()) {
                anyhow::bail!(
                    "cross-cutting commit {} split has overlapping path {:?} between halves — must be disjoint",
                    cc.sha,
                    p
                );
            }
        }
    }
    let missing: Vec<_> = all_paths.difference(&claimed).collect();
    if !missing.is_empty() {
        anyhow::bail!(
            "cross-cutting commit {} split does not cover all paths — missing: {:?}",
            cc.sha,
            missing
        );
    }
    Ok(())
}

fn create_backup_tag(plan: &Plan, plan_yaml: &str) -> Result<BackupTag> {
    let plan_hash = BlakeHash::from_bytes(plan_yaml.as_bytes());
    let now = time::OffsetDateTime::now_utc();
    let stamp = now
        .format(&time::macros::format_description!(
            "[year][month][day]-[hour][minute][second]"
        ))
        .unwrap_or_else(|_| "unknown".into());
    let tag_name = format!("carve-backup/{}-{}", plan.meta.jira_epic, stamp);
    let backup = BackupTag {
        tag_name: tag_name.clone(),
        original_tip: plan.source.tip.clone(),
        original_branch: plan.source.name.clone(),
        plan_hash,
        created_at: now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "unknown".into()),
        carve_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let message = serde_yaml::to_string(&backup).context("serialize backup tag message")?;
    git::create_annotated_tag(&backup.tag_name, &plan.source.tip, &message)?;
    Ok(backup)
}

fn build_node(
    plan: &Plan,
    node: &carve_types::StackNode,
    cross_by_sha: &BTreeMap<&str, &CrossCuttingCommit>,
    force: bool,
) -> Result<()> {
    let already = git::branch_exists(&node.branch);
    if already && !force {
        anyhow::bail!(
            "branch {} already exists locally — pass --force to recreate (origin is unchanged regardless)",
            node.branch
        );
    }
    if already {
        git::branch_force_create(&node.branch, &node.base)?;
    } else {
        git::checkout_new_branch(&node.branch, &node.base)?;
    }

    if node.placeholder {
        let msg = format!(
            "{key}: placeholder — no repo-side scope\n\nGenerated by carve {ver}; see plan for ticket details.",
            key = node.ticket,
            ver = env!("CARGO_PKG_VERSION")
        );
        git::commit_empty(&msg).context("placeholder empty commit")?;
        return Ok(());
    }

    // Build assignment lookup keyed by SHA for this node's commits.
    let assignments_by_sha: BTreeMap<&str, &carve_types::CommitAssignment> = plan
        .assignments
        .iter()
        .map(|a| (a.sha.as_str(), a))
        .collect();
    // Fingerprint lookup for split metadata (author / date / subject).
    let fingerprints_by_sha: BTreeMap<&str, &carve_types::CommitFingerprint> = plan
        .commits
        .iter()
        .map(|c| (c.sha.as_str(), c))
        .collect();

    for sha in &node.commits {
        let sha_str = sha.as_str();
        if let Some(cc) = cross_by_sha.get(sha_str) {
            apply_cross_cutting(cc, node, &fingerprints_by_sha)?;
        } else if let Some(_assn) = assignments_by_sha.get(sha_str) {
            git::cherry_pick(sha_str).with_context(|| {
                format!("cherry-pick {sha_str} onto {}", node.branch)
            })?;
        } else {
            anyhow::bail!(
                "node {} references commit {} that has neither assignment nor cross-cutting entry",
                node.branch,
                sha_str
            );
        }
    }
    Ok(())
}

fn apply_cross_cutting(
    cc: &CrossCuttingCommit,
    node: &carve_types::StackNode,
    fingerprints: &BTreeMap<&str, &carve_types::CommitFingerprint>,
) -> Result<()> {
    let fp = fingerprints
        .get(cc.sha.as_str())
        .ok_or_else(|| anyhow!("cross-cutting commit {} missing fingerprint", cc.sha))?;
    match &cc.decision {
        SplitDecision::Drop { .. } => {
            tracing::warn!(sha = %cc.sha, "dropping cross-cutting commit per plan");
            Ok(())
        }
        SplitDecision::WholeTo { ticket } if *ticket == node.ticket => {
            git::cherry_pick(cc.sha.as_str())?;
            Ok(())
        }
        SplitDecision::WholeTo { .. } => Ok(()), // not for this node
        SplitDecision::ByPath { halves } => {
            // Apply only the half whose ticket matches this node.
            let half = halves.iter().find(|h| h.ticket == node.ticket);
            let Some(half) = half else {
                return Ok(()); // not for this node
            };
            git::apply_commit_slice(cc.sha.as_str(), &half.paths)?;
            let subject = half
                .subject_override
                .clone()
                .unwrap_or_else(|| format!("{} ({} half)", cc.subject, node.ticket));
            let message = format!(
                "{subject}\n\nSplit from original commit {sha} for {ticket} scope.",
                sha = cc.sha,
                ticket = node.ticket,
            );
            git::commit_with_metadata(&half.paths, &message, &fp.author, &fp.author_date)?;
            Ok(())
        }
    }
}

fn default_pr_title(_plan: &Plan, node: &carve_types::StackNode) -> String {
    format!("{}: {}", node.ticket, node.branch)
}

fn default_pr_body(plan: &Plan, node: &carve_types::StackNode) -> String {
    let diagram = render_stack_diagram(plan, Some(&node.branch));
    format!(
        "Generated by `carve` from epic {epic}.\n\n## Stack position\n\n```\n{diagram}\n```\n\nSub-ticket: {tk}\n\n<!-- carve:plan-hash {hash} -->\n",
        epic = plan.meta.jira_epic,
        tk = node.ticket,
        hash = blake_self_hash(plan),
    )
}

pub(crate) fn render_stack_diagram(plan: &Plan, highlight: Option<&str>) -> String {
    let mut lines = Vec::new();
    lines.push(plan.stack.root.clone());
    for (i, n) in plan.stack.nodes.iter().enumerate() {
        let indent = " ".repeat(i * 2 + 1);
        let arrow = "└── ";
        let marker = if Some(n.branch.as_str()) == highlight {
            "   ← you are here"
        } else {
            ""
        };
        lines.push(format!("{indent}{arrow}{}{}", n.branch, marker));
    }
    lines.join("\n")
}

fn blake_self_hash(plan: &Plan) -> String {
    let yaml = plan.to_yaml().unwrap_or_default();
    BlakeHash::from_bytes(yaml.as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use carve_types::{
        commit::CommitSha,
        cross_cutting::{MatchedScope, SplitHalf},
        plan::{PlanMeta, SourceBranch},
        stack::{StackNode, StackTopology},
        ticket::{TicketKey, TicketScope},
    };

    fn make_cc(paths_a: &[&str], paths_b: &[&str], halves: Vec<SplitHalf>) -> CrossCuttingCommit {
        CrossCuttingCommit {
            sha: CommitSha::new("abc1234").unwrap(),
            subject: "test cross-cutter".into(),
            matched_scopes: vec![
                MatchedScope {
                    ticket: TicketKey::new("PROJ-1").unwrap(),
                    paths: paths_a.iter().map(|s| (*s).into()).collect(),
                },
                MatchedScope {
                    ticket: TicketKey::new("PROJ-2").unwrap(),
                    paths: paths_b.iter().map(|s| (*s).into()).collect(),
                },
            ],
            decision: SplitDecision::ByPath { halves },
        }
    }

    #[test]
    fn split_coverage_ok_when_halves_partition_paths() {
        let cc = make_cc(
            &["a.txt"],
            &["b.txt"],
            vec![
                SplitHalf {
                    ticket: TicketKey::new("PROJ-1").unwrap(),
                    paths: vec!["a.txt".into()],
                    subject_override: None,
                },
                SplitHalf {
                    ticket: TicketKey::new("PROJ-2").unwrap(),
                    paths: vec!["b.txt".into()],
                    subject_override: None,
                },
            ],
        );
        if let SplitDecision::ByPath { halves } = &cc.decision {
            assert!(check_split_coverage(&cc, halves).is_ok());
        }
    }

    #[test]
    fn split_coverage_fails_on_overlap() {
        let cc = make_cc(
            &["a.txt"],
            &["b.txt"],
            vec![
                SplitHalf {
                    ticket: TicketKey::new("PROJ-1").unwrap(),
                    paths: vec!["a.txt".into(), "b.txt".into()], // overlap
                    subject_override: None,
                },
                SplitHalf {
                    ticket: TicketKey::new("PROJ-2").unwrap(),
                    paths: vec!["b.txt".into()],
                    subject_override: None,
                },
            ],
        );
        if let SplitDecision::ByPath { halves } = &cc.decision {
            let err = check_split_coverage(&cc, halves).unwrap_err();
            assert!(err.to_string().contains("overlapping"), "got: {err}");
        }
    }

    #[test]
    fn split_coverage_fails_on_missing_path() {
        let cc = make_cc(
            &["a.txt"],
            &["b.txt"],
            vec![SplitHalf {
                ticket: TicketKey::new("PROJ-1").unwrap(),
                paths: vec!["a.txt".into()],
                subject_override: None,
            }], // b.txt never claimed
        );
        if let SplitDecision::ByPath { halves } = &cc.decision {
            let err = check_split_coverage(&cc, halves).unwrap_err();
            assert!(err.to_string().contains("missing"), "got: {err}");
        }
    }

    #[test]
    fn stack_diagram_marks_highlight() {
        let plan = Plan {
            meta: PlanMeta {
                carve_version: "0.1.0".into(),
                generated_at: "now".into(),
                jira_epic: "PROJ-1".into(),
                operator: "test".into(),
            },
            source: SourceBranch {
                name: "feat".into(),
                master_branch: "master".into(),
                tip: "abc".into(),
                merge_base: "def".into(),
            },
            tickets: vec![
                TicketScope {
                    key: TicketKey::new("PROJ-1").unwrap(),
                    summary: "first".into(),
                    paths: vec![],
                    exclude: vec![],
                    placeholder: false,
                    stack_order: 0,
                    story_points: None,
                    target_status: None,
                },
                TicketScope {
                    key: TicketKey::new("PROJ-2").unwrap(),
                    summary: "second".into(),
                    paths: vec![],
                    exclude: vec![],
                    placeholder: false,
                    stack_order: 10,
                    story_points: None,
                    target_status: None,
                },
            ],
            commits: vec![],
            assignments: vec![],
            cross_cutting: vec![],
            stack: StackTopology {
                root: "master".into(),
                nodes: vec![
                    StackNode {
                        ticket: TicketKey::new("PROJ-1").unwrap(),
                        branch: "feat-a".into(),
                        base: "master".into(),
                        commits: vec![],
                        placeholder: false,
                        pr_title: None,
                        pr_body: None,
                        draft: false,
                    },
                    StackNode {
                        ticket: TicketKey::new("PROJ-2").unwrap(),
                        branch: "feat-b".into(),
                        base: "feat-a".into(),
                        commits: vec![],
                        placeholder: false,
                        pr_title: None,
                        pr_body: None,
                        draft: false,
                    },
                ],
            },
        };
        let diagram = render_stack_diagram(&plan, Some("feat-b"));
        assert!(diagram.contains("master"));
        assert!(diagram.contains("feat-a"));
        assert!(diagram.contains("feat-b"));
        assert!(diagram.contains("← you are here"));
        // Only `feat-b` should have the marker.
        let with_marker_count = diagram.matches("← you are here").count();
        assert_eq!(with_marker_count, 1);
    }
}
