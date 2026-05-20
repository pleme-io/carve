//! Git helpers — shell-out to the system `git` binary.
//!
//! Carve deliberately uses shell-out over libgit2 bindings because we need
//! to pipe `git show <sha> -- <paths> | git apply` for the cross-cutting
//! commit-splitting workflow, which libgit2 doesn't expose ergonomically.

use anyhow::{anyhow, Context, Result};
use carve_types::{CommitFingerprint, commit::CommitSha};
use std::process::{Command, Stdio};

/// Run a git command, capturing stdout. Empty stdout is fine; any non-zero
/// exit code is an error containing the stderr.
pub fn git(args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("spawn git {:?}", args))?;
    if !out.status.success() {
        return Err(anyhow!(
            "git {:?} failed (exit {}): {}",
            args,
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8(out.stdout).context("git stdout was not utf-8")?)
}

/// Resolve a rev (branch / tag / sha) to its full 40-char SHA.
pub fn resolve(rev: &str) -> Result<String> {
    Ok(git(&["rev-parse", rev])?.trim().to_string())
}

/// Current branch name (`HEAD`). Returns an error in detached-HEAD state.
pub fn current_branch() -> Result<String> {
    let out = git(&["symbolic-ref", "--short", "HEAD"])?;
    Ok(out.trim().to_string())
}

/// Merge-base of two revs.
pub fn merge_base(a: &str, b: &str) -> Result<String> {
    Ok(git(&["merge-base", a, b])?.trim().to_string())
}

/// Branch-local work commits in the range `base..tip`, **chronological**
/// (oldest first).
///
/// Filters out merge commits (`--no-merges`) and only follows the first
/// parent of any merges (`--first-parent`) so commits that arrived via a
/// `merge from master` aren't double-counted. The result is the set of
/// commits authored *on this branch* — the right set for cherry-picking
/// into a derived stack.
pub fn commits_in_range(base: &str, tip: &str) -> Result<Vec<String>> {
    let out = git(&[
        "log",
        "--first-parent",
        "--no-merges",
        "--reverse",
        "--format=%H",
        &format!("{base}..{tip}"),
    ])?;
    Ok(out.lines().map(|s| s.to_string()).collect())
}

/// Full fingerprint of a single commit: subject, paths, author, date.
pub fn fingerprint(sha: &str) -> Result<CommitFingerprint> {
    // Use NUL-delimited format to make subject + author safe.
    let raw = git(&[
        "show",
        "--no-patch",
        "--format=%H%x00%s%x00%ae%x00%aI",
        sha,
    ])?;
    let line = raw.trim_end_matches('\n');
    let mut parts = line.split('\0');
    let parsed_sha = parts.next().context("missing sha")?.to_string();
    let subject = parts.next().context("missing subject")?.to_string();
    let author = parts.next().context("missing author")?.to_string();
    let author_date = parts.next().context("missing author date")?.to_string();
    let paths = paths_touched(sha)?;
    Ok(CommitFingerprint {
        sha: CommitSha::new(parsed_sha)?,
        subject,
        paths,
        author,
        author_date,
    })
}

/// Files touched by a commit (added / modified / deleted / renamed).
pub fn paths_touched(sha: &str) -> Result<Vec<String>> {
    // --name-only on `git show` includes both sides of renames as separate
    // lines; that's what we want — both old and new path counted as touched.
    let raw = git(&["show", "--name-only", "--format=", sha])?;
    Ok(raw
        .lines()
        .filter(|l| !l.is_empty())
        .map(|s| s.to_string())
        .collect())
}

/// Tree hash of a rev — for the post-execute tree-equivalence gate.
pub fn tree_hash(rev: &str) -> Result<String> {
    Ok(git(&["rev-parse", &format!("{rev}^{{tree}}")])?.trim().to_string())
}

/// Repo root (absolute path).
pub fn repo_root() -> Result<std::path::PathBuf> {
    let out = git(&["rev-parse", "--show-toplevel"])?;
    Ok(std::path::PathBuf::from(out.trim()))
}

/// The remote's default branch, fully qualified (e.g. `origin/main` or
/// `origin/master`). Resolved by reading `refs/remotes/origin/HEAD` — the
/// canonical, repo-aware answer that handles main/master/trunk/etc.
/// uniformly. Falls back to probing `origin/main` then `origin/master`
/// in case `origin/HEAD` is unset on a freshly-cloned repo.
pub fn default_remote_branch() -> Result<String> {
    if let Ok(out) = git(&["symbolic-ref", "refs/remotes/origin/HEAD"]) {
        let raw = out.trim();
        // strip the `refs/remotes/` prefix → `origin/main`
        return Ok(raw.trim_start_matches("refs/remotes/").to_string());
    }
    for cand in &["origin/main", "origin/master"] {
        if resolve(cand).is_ok() {
            return Ok((*cand).to_string());
        }
    }
    anyhow::bail!(
        "could not determine default remote branch — set `git remote set-head origin --auto` or pass --master explicitly"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// We can't unit-test fingerprint without a real repo; integration
    /// tests against the ASM-18003 fixture cover that. But we *can* verify
    /// the git binary is on PATH in dev environments.
    #[test]
    #[ignore = "requires git on PATH"]
    fn git_is_callable() {
        let out = git(&["--version"]).expect("git --version");
        assert!(out.starts_with("git version"));
    }
}
