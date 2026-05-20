//! Git helpers — shell-out to the system `git` binary.
//!
//! Carve deliberately uses shell-out over libgit2 bindings because we need
//! to pipe `git show <sha> -- <paths> | git apply` for the cross-cutting
//! commit-splitting workflow, which libgit2 doesn't expose ergonomically.

use anyhow::{anyhow, Context, Result};
use carve_types::{CommitFingerprint, commit::CommitSha};
use std::io::Write;
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

/// Check out a new branch starting at `start_point`. Fails if the branch
/// already exists (use [`branch_force_create`] for the recreate case).
pub fn checkout_new_branch(name: &str, start_point: &str) -> Result<()> {
    git(&["checkout", "-b", name, start_point])?;
    Ok(())
}

/// Force-create a branch at `start_point`, discarding the previous tip if
/// it existed. Used by `carve execute --force`.
pub fn branch_force_create(name: &str, start_point: &str) -> Result<()> {
    git(&["checkout", "-B", name, start_point])?;
    Ok(())
}

/// Does a local branch with this name exist?
pub fn branch_exists(name: &str) -> bool {
    git(&["show-ref", "--quiet", &format!("refs/heads/{name}")]).is_ok()
}

/// Cherry-pick a commit onto the current branch. Returns an error
/// containing the conflicting paths if cherry-pick fails — the caller
/// should `cherry-pick --abort` and surface the conflict.
pub fn cherry_pick(sha: &str) -> Result<()> {
    git(&["cherry-pick", sha]).with_context(|| format!("cherry-pick {sha}"))?;
    Ok(())
}

/// Abort an in-progress cherry-pick.
#[allow(dead_code)]
pub fn cherry_pick_abort() -> Result<()> {
    // Don't fail if there's nothing to abort; some callers use this as
    // belt-and-suspenders cleanup.
    let _ = git(&["cherry-pick", "--abort"]);
    Ok(())
}

/// Apply just the path-restricted slice of a commit's diff. Used for
/// cross-cutting commit splits — runs `git show <sha> -- <paths> | git apply`.
/// The diff is path-filtered by git itself, so only hunks for the
/// requested paths are emitted.
pub fn apply_commit_slice(sha: &str, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        anyhow::bail!("apply_commit_slice: paths must be non-empty");
    }
    // Build args: git show <sha> -- <paths...>
    let mut show_args: Vec<&str> = vec!["show", sha, "--"];
    for p in paths {
        show_args.push(p);
    }
    let show = Command::new("git")
        .args(&show_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn git show")?;
    let show_out = show.wait_with_output().context("wait git show")?;
    if !show_out.status.success() {
        anyhow::bail!(
            "git show {sha} -- {:?} failed: {}",
            paths,
            String::from_utf8_lossy(&show_out.stderr).trim()
        );
    }
    if show_out.stdout.is_empty() {
        anyhow::bail!(
            "git show {sha} -- {:?} produced empty diff — paths may not match anything",
            paths
        );
    }

    // Pipe the diff into `git apply`.
    let mut apply = Command::new("git")
        .args(["apply"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn git apply")?;
    apply
        .stdin
        .as_mut()
        .context("apply stdin")?
        .write_all(&show_out.stdout)?;
    let apply_out = apply.wait_with_output().context("wait git apply")?;
    if !apply_out.status.success() {
        anyhow::bail!(
            "git apply (slice of {sha}) failed: {}",
            String::from_utf8_lossy(&apply_out.stderr).trim()
        );
    }
    Ok(())
}

/// Stage all changes under the given paths and commit with the supplied
/// metadata. Used after [`apply_commit_slice`] to commit a split half.
pub fn commit_with_metadata(
    paths: &[String],
    message: &str,
    author: &str,
    author_date: &str,
) -> Result<String> {
    // Stage only the paths the split actually touched.
    if paths.is_empty() {
        anyhow::bail!("commit_with_metadata: paths must be non-empty");
    }
    let mut add_args: Vec<&str> = vec!["add", "--"];
    for p in paths {
        add_args.push(p);
    }
    git(&add_args).context("git add")?;
    git(&[
        "-c",
        "core.editor=true",
        "commit",
        "-m",
        message,
        "--author",
        author,
        "--date",
        author_date,
    ])
    .context("git commit")?;
    Ok(git(&["rev-parse", "HEAD"])?.trim().to_string())
}

/// Create an empty `--allow-empty` commit on the current branch. Used for
/// placeholder ticket nodes that have no repo-side scope.
pub fn commit_empty(message: &str) -> Result<String> {
    git(&[
        "-c",
        "core.editor=true",
        "commit",
        "--allow-empty",
        "-m",
        message,
    ])?;
    Ok(git(&["rev-parse", "HEAD"])?.trim().to_string())
}

/// Annotated tag with a multi-line message. `target` may be a SHA or ref.
pub fn create_annotated_tag(name: &str, target: &str, message: &str) -> Result<()> {
    git(&["tag", "-a", name, target, "-m", message])?;
    Ok(())
}

/// Push a list of refs to origin. Uses `--force-with-lease` so we never
/// clobber upstream work we didn't see — a fresh fetch is required if
/// someone else updated the same branch.
pub fn push_with_lease(refs: &[&str]) -> Result<String> {
    let mut args: Vec<&str> = vec!["push", "--force-with-lease", "origin"];
    for r in refs {
        args.push(r);
    }
    git(&args)
}

/// Push a tag to origin. Tags are append-only — no force needed.
pub fn push_tag(name: &str) -> Result<String> {
    git(&["push", "origin", name])
}

/// Whether the working tree (and index) has any modifications. Used by
/// execute to refuse to start in a dirty repo.
pub fn working_tree_clean() -> Result<bool> {
    let out = git(&["status", "--porcelain"])?;
    Ok(out.trim().is_empty())
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
