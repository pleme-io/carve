//! GitHub helpers — shells out to `gh` for PR creation / editing, uses
//! octocrab for read-side queries that benefit from typing.
//!
//! Auth: relies on the `gh` CLI being authenticated; octocrab reads
//! `GH_TOKEN` / `GITHUB_TOKEN` from env. Both rest on operator-level
//! authentication; carve never asks for credentials directly.

use anyhow::{Context, Result};
use std::process::Command;

pub fn create_pr(
    base: &str,
    head: &str,
    title: &str,
    body: &str,
    draft: bool,
) -> Result<String> {
    let mut cmd = Command::new("gh");
    cmd.args(["pr", "create", "--base", base, "--head", head, "--title", title, "--body", body]);
    if draft {
        cmd.arg("--draft");
    }
    let out = cmd.output().context("spawn gh pr create")?;
    if !out.status.success() {
        anyhow::bail!(
            "gh pr create failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let url = String::from_utf8(out.stdout)?.trim().to_string();
    Ok(url)
}

pub fn edit_pr_body(pr: u64, body: &str) -> Result<()> {
    let out = Command::new("gh")
        .args(["pr", "edit", &pr.to_string(), "--body", body])
        .output()
        .context("spawn gh pr edit")?;
    if !out.status.success() {
        anyhow::bail!(
            "gh pr edit failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Read a PR's current body (markdown).
pub fn pr_body(pr: u64) -> Result<String> {
    let out = Command::new("gh")
        .args(["pr", "view", &pr.to_string(), "--json", "body", "--jq", ".body"])
        .output()
        .context("spawn gh pr view")?;
    if !out.status.success() {
        anyhow::bail!(
            "gh pr view {} failed: {}",
            pr,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?.trim_end_matches('\n').to_string())
}

/// Read a PR's state (`OPEN` / `CLOSED` / `MERGED`).
pub fn pr_state(pr: u64) -> Result<String> {
    let out = Command::new("gh")
        .args(["pr", "view", &pr.to_string(), "--json", "state", "--jq", ".state"])
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

/// Find the open PR (if any) for a given head branch.
pub fn pr_for_branch(branch: &str) -> Result<Option<u64>> {
    let out = Command::new("gh")
        .args([
            "pr", "list", "--head", branch, "--state", "all",
            "--json", "number,state", "--jq", ".[0].number // \"\"",
        ])
        .output()
        .context("spawn gh pr list")?;
    if !out.status.success() {
        anyhow::bail!(
            "gh pr list --head {} failed: {}",
            branch,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let s = String::from_utf8(out.stdout)?.trim().to_string();
    if s.is_empty() || s == "\"\"" {
        return Ok(None);
    }
    Ok(Some(s.parse()?))
}
