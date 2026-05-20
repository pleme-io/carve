//! Integration test: spin up a synthetic git repo, build a Plan that
//! carves its commits across 2 tickets, run `carve execute` with
//! `--no-push`, and verify the resulting branches + tree-hash gate.
//!
//! This is the real-fixture test that runs anywhere (CI, dev laptop,
//! offline) and guards the execute pipeline against regressions.
//!
//! For the live ASM-18003 fixture (which depends on the akeyless
//! workstation having that repo checked out and the
//! `asm18003-pre-split-backup` tag present), see the `#[ignore]`d test
//! at the bottom — run it manually with:
//!
//! ```sh
//! cargo test --test synthetic_repo -- --ignored asm18003_live
//! ```

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn run(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn git");
    if !out.status.success() {
        panic!(
            "git {:?} in {:?} failed: {}",
            args,
            dir,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8(out.stdout).expect("utf8 git stdout")
}

fn write_file(dir: &Path, rel: &str, contents: &str) {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, contents).unwrap();
}

fn build_synthetic_repo() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path();

    // git init main
    run(p, &["init", "-b", "main"]);
    run(p, &["config", "user.email", "test@carve.dev"]);
    run(p, &["config", "user.name", "Carve Test"]);

    // Base commit (will become the "master tip" for the test).
    write_file(p, "README.md", "# project\n");
    run(p, &["add", "README.md"]);
    run(p, &["commit", "-m", "initial: project skeleton"]);

    // Tag this as our "remote master" so plan/execute can resolve it via
    // a fake origin/master ref.
    run(p, &["tag", "synthetic-master"]);
    run(p, &["update-ref", "refs/remotes/origin/master", "HEAD"]);
    // Set origin/HEAD so default_remote_branch() works.
    run(p, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/master"]);

    // Build a feature branch with 3 commits — one per ticket scope.
    run(p, &["checkout", "-b", "feat-monolithic"]);

    // Commit 1: backend-only.
    write_file(p, "backend/api.rs", "fn handler() {}\n");
    run(p, &["add", "backend/api.rs"]);
    run(p, &["commit", "-m", "PROJ-1: add backend API handler"]);

    // Commit 2: frontend-only.
    write_file(p, "frontend/app.tsx", "export const App = () => null;\n");
    run(p, &["add", "frontend/app.tsx"]);
    run(p, &["commit", "-m", "PROJ-2: add frontend App component"]);

    // Commit 3: docs-only.
    write_file(p, "docs/README.md", "# docs\n");
    run(p, &["add", "docs/README.md"]);
    run(p, &["commit", "-m", "PROJ-3: add docs README"]);

    dir
}

#[test]
fn synthetic_repo_three_ticket_carve() {
    let tmp = build_synthetic_repo();
    let p = tmp.path();

    let tip = run(p, &["rev-parse", "HEAD"]).trim().to_string();
    let master = run(p, &["rev-parse", "origin/master"]).trim().to_string();
    let log = run(p, &["log", "--reverse", "--format=%H %s", &format!("{master}..{tip}")]);
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 3, "expected 3 commits between master and tip");

    let shas: Vec<&str> = lines.iter().map(|l| l.split(' ').next().unwrap()).collect();
    // Sanity: the three SHAs differ (regression for any accidental fixed-point).
    assert_eq!(shas.iter().collect::<std::collections::HashSet<_>>().len(), 3);

    // Build the plan in YAML so we can serialise and deserialise. (We
    // construct it programmatically rather than via `carve plan` because
    // plan currently emits an empty `tickets` list without a JIRA client.)
    let plan_yaml = format!(
        r#"meta:
  carve_version: 0.1.0
  generated_at: 2026-01-01T00:00:00Z
  jira_epic: PROJ-0
  operator: test@carve.dev
source:
  name: feat-monolithic
  master_branch: origin/master
  tip: {tip}
  merge_base: {master}
tickets:
- key: PROJ-1
  summary: backend
  paths: ['backend/**']
  exclude: []
  placeholder: false
  stack_order: 0
- key: PROJ-2
  summary: frontend
  paths: ['frontend/**']
  exclude: []
  placeholder: false
  stack_order: 10
- key: PROJ-3
  summary: docs
  paths: ['docs/**']
  exclude: []
  placeholder: false
  stack_order: 20
commits:
- sha: {sha0}
  subject: 'PROJ-1: add backend API handler'
  paths: ['backend/api.rs']
  author: test@carve.dev
  author_date: 2026-01-01T00:00:00Z
- sha: {sha1}
  subject: 'PROJ-2: add frontend App component'
  paths: ['frontend/app.tsx']
  author: test@carve.dev
  author_date: 2026-01-01T00:01:00Z
- sha: {sha2}
  subject: 'PROJ-3: add docs README'
  paths: ['docs/README.md']
  author: test@carve.dev
  author_date: 2026-01-01T00:02:00Z
assignments:
- sha: {sha0}
  ticket: PROJ-1
  confidence: high-single-scope
- sha: {sha1}
  ticket: PROJ-2
  confidence: high-single-scope
- sha: {sha2}
  ticket: PROJ-3
  confidence: high-single-scope
cross_cutting: []
stack:
  root: origin/master
  nodes:
  - ticket: PROJ-1
    branch: PROJ-1-backend
    base: origin/master
    commits: ['{sha0}']
    placeholder: false
    draft: false
  - ticket: PROJ-2
    branch: PROJ-2-frontend
    base: PROJ-1-backend
    commits: ['{sha1}']
    placeholder: false
    draft: false
  - ticket: PROJ-3
    branch: PROJ-3-docs
    base: PROJ-2-frontend
    commits: ['{sha2}']
    placeholder: false
    draft: false
"#,
        tip = tip,
        master = master,
        sha0 = shas[0],
        sha1 = shas[1],
        sha2 = shas[2],
    );
    // Plan file lives OUTSIDE the repo so it doesn't show up as untracked
    // and trip working_tree_clean's dirty-tree check.
    let plan_holder = tempfile::tempdir().expect("plan tempdir");
    let plan_path = plan_holder.path().join("plan.yaml");
    std::fs::write(&plan_path, &plan_yaml).unwrap();

    // Run `carve execute --no-push`. We use the binary built by cargo
    // test's parent invocation — env!("CARGO_BIN_EXE_carve") is populated
    // by cargo for integration tests.
    let bin = env!("CARGO_BIN_EXE_carve");
    let out = Command::new(bin)
        .args([
            "execute",
            "--plan",
            plan_path.to_str().unwrap(),
            "--no-push",
        ])
        .current_dir(p)
        .output()
        .expect("spawn carve");
    assert!(
        out.status.success(),
        "carve execute failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("tree-hash gate PASSED"), "expected tree-hash gate to pass: {stdout}");

    // Verify the 3 branches exist locally.
    for branch in ["PROJ-1-backend", "PROJ-2-frontend", "PROJ-3-docs"] {
        let sha = run(p, &["rev-parse", branch]);
        assert!(!sha.trim().is_empty(), "branch {branch} should exist");
    }

    // Verify the stack-top tree equals the source-branch tree.
    let source_tree = run(p, &["rev-parse", &format!("{tip}^{{tree}}")]).trim().to_string();
    let top_tree = run(p, &["rev-parse", "PROJ-3-docs^{tree}"]).trim().to_string();
    assert_eq!(source_tree, top_tree, "tree-hash equivalence");

    // Verify a backup tag was created and points at the source tip.
    let tags = run(p, &["tag", "-l", "carve-backup/*"]);
    assert!(!tags.trim().is_empty(), "expected at least one carve-backup tag");
    let backup_tag_name = tags.lines().next().unwrap().trim();
    let backup_target = run(p, &["rev-parse", backup_tag_name]).trim().to_string();
    let backup_commit = run(p, &["rev-list", "-1", backup_tag_name]).trim().to_string();
    // The tag is annotated, so rev-parse gives the tag object; rev-list -1 gives the commit.
    assert_eq!(backup_commit, tip, "backup tag should target the source tip");
    assert_ne!(backup_target, backup_commit, "annotated tag should differ from its commit target");
}

#[test]
#[ignore = "depends on akeyless-environments worktree + asm18003-pre-split-backup tag"]
fn asm18003_live() {
    // Manual fixture: confirms that against the real akeyless-environments
    // ASM-18003 branch + the asm18003-pre-split-backup tag, `carve plan`
    // captures exactly the 45 work commits.
    //
    // Manual run:
    //   ATLASSIAN_BASE_URL=... ATLASSIAN_EMAIL=... ATLASSIAN_API_TOKEN=... \
    //     cargo test --test synthetic_repo -- --ignored asm18003_live
    let repo = std::env::var("ASM18003_REPO_PATH")
        .unwrap_or_else(|_| "/Users/luis.d/code/github/akeylesslabs/akeyless-environments-asm18003".to_string());
    let bin = env!("CARGO_BIN_EXE_carve");
    let out = Command::new(bin)
        .args(["plan", "--epic", "ASM-18003", "--out", "/tmp/asm18003-fixture.yaml"])
        .current_dir(&repo)
        .output()
        .expect("spawn carve");
    assert!(out.status.success(), "carve plan failed: {}", String::from_utf8_lossy(&out.stderr));
    let yaml = std::fs::read_to_string("/tmp/asm18003-fixture.yaml").unwrap();
    let commit_count = yaml.matches("\n- sha:").count();
    assert_eq!(commit_count, 45, "expected exactly 45 ASM-18003 work commits");
    assert!(yaml.contains("DBK staging asia-southeast1 read region TF scaffolding"), "first commit subject missing");
    assert!(yaml.contains("single-canary policy"), "last commit subject missing");
}
