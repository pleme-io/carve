//! carve — monolithic branch → ticket-aligned stacked PR primitive.
//!
//! See `carve --help` for the full subcommand surface.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod cmd;
mod git;
mod github;
mod jira;

#[derive(Debug, Parser)]
#[command(name = "carve")]
#[command(version, about = "Monolithic branch → ticket-aligned stacked-PR carving CLI", long_about = None)]
struct Cli {
    /// Logging verbosity (-v info, -vv debug, -vvv trace).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Analyse the current branch and emit a plan.yaml the operator can
    /// hand-edit before execution.
    Plan {
        /// JIRA epic key (e.g. ASM-18003). Sub-tickets are fetched
        /// automatically.
        #[arg(long)]
        epic: String,
        /// Branch to carve. Defaults to the current branch.
        #[arg(long)]
        branch: Option<String>,
        /// Ref the stack should ultimately root on. Default
        /// `origin/master` because operators typically work in worktrees
        /// fetched off the remote and the local `master` is often stale.
        /// Pass `--master main` (or `--master origin/main`) for other
        /// repos.
        #[arg(long, default_value = "origin/master")]
        master: String,
        /// Output path for the generated plan.yaml.
        #[arg(long, short, default_value = "plan.yaml")]
        out: PathBuf,
    },

    /// Verify a plan.yaml without mutating anything — dry-run, checks all
    /// invariants, reports cross-cutting commits that need decisions.
    Verify {
        #[arg(long, short, default_value = "plan.yaml")]
        plan: PathBuf,
    },

    /// Execute a plan.yaml — creates the backup tag, builds branches,
    /// verifies tree-hash equivalence, pushes, and opens stacked PRs.
    Execute {
        #[arg(long, short, default_value = "plan.yaml")]
        plan: PathBuf,
        /// Skip the push + PR creation step (build branches only).
        #[arg(long)]
        no_push: bool,
        /// Force-create branches even if they exist locally.
        #[arg(long)]
        force: bool,
    },

    /// Sync JIRA per the plan: story points + transition + ADF comment
    /// with PR link, for every ticket that has a stack node.
    JiraSync {
        #[arg(long, short, default_value = "plan.yaml")]
        plan: PathBuf,
        /// Skip story-point updates (transitions only).
        #[arg(long)]
        no_points: bool,
        /// Skip status transitions (story-point updates only).
        #[arg(long)]
        no_transition: bool,
    },

    /// After a fix lands on a parent PR, replay the descendants on top
    /// of the new tip. git-branchless-style. Tree-hash gate applies.
    Restack {
        #[arg(long, short, default_value = "plan.yaml")]
        plan: PathBuf,
        /// The branch whose tip moved (the fix landed here).
        #[arg(long)]
        from: String,
    },

    /// Regenerate the embedded ASCII stack diagram in each PR body.
    /// Idempotent — only touches content inside the carve-managed fence.
    Diagram {
        #[arg(long, short, default_value = "plan.yaml")]
        plan: PathBuf,
    },

    /// CI hook: refuse to merge if a parent PR in the stack is still open.
    Gate {
        /// PR number under check.
        #[arg(long)]
        pr: u64,
        #[arg(long, short, default_value = "plan.yaml")]
        plan: PathBuf,
    },

    /// Show stack health: base-chain drift, parent-merge state, JIRA
    /// status divergence.
    Status {
        #[arg(long, short, default_value = "plan.yaml")]
        plan: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    tracing::debug!(?cli, "parsed CLI");

    match cli.command {
        Command::Plan {
            epic,
            branch,
            master,
            out,
        } => cmd::plan::run(cmd::plan::Args {
            epic,
            branch,
            master,
            out,
        })
        .context("carve plan"),
        Command::Verify { plan } => cmd::verify::run(plan).context("carve verify"),
        Command::Execute {
            plan,
            no_push,
            force,
        } => cmd::execute::run(cmd::execute::Args {
            plan,
            no_push,
            force,
        })
        .context("carve execute"),
        Command::JiraSync {
            plan,
            no_points,
            no_transition,
        } => cmd::jira_sync::run(cmd::jira_sync::Args {
            plan,
            no_points,
            no_transition,
        })
        .context("carve jira-sync"),
        Command::Restack { plan, from } => {
            cmd::restack::run(cmd::restack::Args { plan, from }).context("carve restack")
        }
        Command::Diagram { plan } => cmd::diagram::run(plan).context("carve diagram"),
        Command::Gate { pr, plan } => {
            cmd::gate::run(cmd::gate::Args { pr, plan }).context("carve gate")
        }
        Command::Status { plan } => cmd::status::run(plan).context("carve status"),
    }
}

fn init_tracing(verbosity: u8) {
    use tracing_subscriber::EnvFilter;
    let level = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("carve={level}")));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}
