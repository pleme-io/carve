//! Stack topology — branches, base chain, dependency order.

use crate::commit::CommitSha;
use crate::ticket::TicketKey;
use serde::{Deserialize, Serialize};

/// A single node in the stack — one branch, one PR, one ticket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackNode {
    pub ticket: TicketKey,
    /// Branch name on origin. Convention: `<TICKET>-<short-slug>`.
    pub branch: String,
    /// Base branch this PR targets. The bottom of the stack targets
    /// `master`/`main`; every subsequent node targets the prior node's
    /// branch.
    pub base: String,
    /// The commits, in chronological order (matching the original branch),
    /// that will be cherry-picked onto this branch. For cross-cutting
    /// commits the SHA refers to the original; carve resolves the split at
    /// execute time using the cross_cutting table.
    pub commits: Vec<CommitSha>,
    /// True for tickets with no repo-side scope. carve execute writes a
    /// single `--allow-empty` commit with the placeholder message; the
    /// resulting PR is opened in draft state.
    pub placeholder: bool,
    /// Optional override for the generated PR title. If `None`, carve
    /// renders `<TICKET>: <ticket summary>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_title: Option<String>,
    /// Optional override for the generated PR body. If `None`, carve
    /// renders the standard template (summary + stack-position diagram +
    /// linked tickets + test-plan stub).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_body: Option<String>,
    /// Open PR as draft. Auto-true for placeholder nodes; operator-set
    /// otherwise.
    #[serde(default)]
    pub draft: bool,
}

/// The full ordered list of stack nodes. First entry is the base
/// (rooted on master); subsequent entries chain on the prior node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackTopology {
    /// The branch every node ultimately roots on. Usually `master` or `main`.
    pub root: String,
    pub nodes: Vec<StackNode>,
}
