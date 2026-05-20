//! `carve diagram` — idempotent ASCII stack-diagram refresh inside each
//! PR body.
//!
//! Each PR body is expected to contain an HTML-comment fence:
//!
//! ```text
//! <!-- carve:stack-diagram begin -->
//! ...whatever — carve overwrites this region in place...
//! <!-- carve:stack-diagram end -->
//! ```
//!
//! If the fence is absent, carve appends one to the end of the body
//! rather than rewriting unrelated content. Operators who hand-edit a PR
//! body outside the fence keep their edits.

use crate::cmd::execute;
use crate::{git, github};
use anyhow::{Context, Result};
use carve_types::Plan;
use colored::Colorize;
use std::path::PathBuf;

const FENCE_BEGIN: &str = "<!-- carve:stack-diagram begin -->";
const FENCE_END: &str = "<!-- carve:stack-diagram end -->";

pub fn run(plan_path: PathBuf) -> Result<()> {
    let yaml = std::fs::read_to_string(&plan_path)
        .with_context(|| format!("read {}", plan_path.display()))?;
    let plan = Plan::from_yaml(&yaml).context("parse plan.yaml")?;
    plan.check_basic_invariants()?;

    for node in &plan.stack.nodes {
        let Some(pr) = lookup_pr_for_branch(&node.branch)? else {
            tracing::warn!(branch = %node.branch, "no PR found for branch; skipping");
            continue;
        };
        let old_body = github::pr_body(pr).with_context(|| format!("read PR #{pr} body"))?;
        let diagram = execute::render_stack_diagram(&plan, Some(&node.branch));
        let new_body = replace_or_append(&old_body, &diagram);
        if new_body == old_body {
            println!("{} PR #{pr} ({}) already up-to-date", "·".dimmed(), node.branch);
            continue;
        }
        github::edit_pr_body(pr, &new_body)?;
        println!(
            "{} PR #{pr} ({}) diagram refreshed",
            "✓".green().bold(),
            node.branch.bold()
        );
    }
    Ok(())
}

fn replace_or_append(body: &str, diagram: &str) -> String {
    let block = format!(
        "{FENCE_BEGIN}\n```\n{diagram}\n```\n{FENCE_END}"
    );
    if let (Some(begin), Some(end)) = (body.find(FENCE_BEGIN), body.find(FENCE_END)) {
        if begin < end {
            let mut out = String::with_capacity(body.len());
            out.push_str(&body[..begin]);
            out.push_str(&block);
            out.push_str(&body[end + FENCE_END.len()..]);
            return out;
        }
    }
    // No fence — append, separated by a blank line.
    let mut out = body.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(&block);
    out.push('\n');
    out
}

fn lookup_pr_for_branch(branch: &str) -> Result<Option<u64>> {
    // Use `gh pr list --head <branch>` to find the open PR for this
    // branch. Returns the first match if any.
    let out = git::git(&[
        "rev-parse",
        "--verify",
        &format!("refs/heads/{branch}"),
    ]);
    if out.is_err() {
        // local branch missing isn't fatal — gh can still find the PR via origin
        tracing::debug!(branch = %branch, "no local branch ref; querying gh anyway");
    }
    let raw = std::process::Command::new("gh")
        .args([
            "pr", "list", "--head", branch, "--state", "open",
            "--json", "number", "--jq", ".[0].number // \"\"",
        ])
        .output()
        .context("spawn gh pr list")?;
    if !raw.status.success() {
        anyhow::bail!(
            "gh pr list --head {branch} failed: {}",
            String::from_utf8_lossy(&raw.stderr).trim()
        );
    }
    let trimmed = String::from_utf8(raw.stdout)?.trim().to_string();
    if trimmed.is_empty() || trimmed == "\"\"" {
        return Ok(None);
    }
    Ok(Some(trimmed.parse()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_inside_existing_fence() {
        let body = "header\n\n<!-- carve:stack-diagram begin -->\n```\nold\n```\n<!-- carve:stack-diagram end -->\n\nfooter\n";
        let out = replace_or_append(body, "new");
        assert!(out.contains("header"));
        assert!(out.contains("footer"));
        assert!(out.contains("new"));
        assert!(!out.contains("old"));
    }

    #[test]
    fn appends_when_fence_missing() {
        let body = "body without fence";
        let out = replace_or_append(body, "diagram");
        assert!(out.starts_with("body without fence"));
        assert!(out.contains(FENCE_BEGIN));
        assert!(out.contains(FENCE_END));
        assert!(out.contains("diagram"));
    }
}
