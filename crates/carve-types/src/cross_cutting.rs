//! Cross-cutting commits and their split decisions.
//!
//! A commit is **cross-cutting** when the paths it touches fall under more
//! than one [`crate::TicketScope`]. Carve detects these automatically and
//! emits a [`CrossCuttingCommit`] entry in the plan; the operator either
//! accepts the proposed split or hand-edits the path partitioning. Carve
//! never silently merges a cross-cutting commit into one ticket — every
//! split is explicit.

use crate::commit::CommitSha;
use crate::ticket::TicketKey;
use serde::{Deserialize, Serialize};

/// One half of a split. After execution, each half becomes its own commit
/// in the target ticket's branch, preserving the original author + date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitHalf {
    pub ticket: TicketKey,
    /// Glob patterns identifying the paths claimed by this half. At
    /// execute time, carve runs `git show <sha> -- <paths> | git apply`
    /// to materialise just this slice.
    pub paths: Vec<String>,
    /// Subject line for the derived commit. If omitted, carve uses the
    /// original subject with a " (<ticket> half)" suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_override: Option<String>,
}

/// How carve will split a cross-cutting commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SplitDecision {
    /// Split by path globs — each half claims a disjoint subset of paths.
    /// `halves` must collectively cover all paths the commit touched (no
    /// drift) and must not overlap (no double-application).
    ByPath { halves: Vec<SplitHalf> },
    /// Operator override: assign the entire commit to a single ticket
    /// without splitting. Use sparingly; defeats the cross-cutting
    /// safeguard and accepts blurred ticket scope as the cost.
    WholeTo { ticket: TicketKey },
    /// Drop the commit entirely from the stack. Carve refuses to execute
    /// this unless the operator also adds an annotation explaining why
    /// (rationale field non-empty).
    Drop { rationale: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossCuttingCommit {
    pub sha: CommitSha,
    pub subject: String,
    /// All paths this commit touches, grouped by the ticket whose scope
    /// claims them. Used by carve verify to render a per-ticket diff
    /// preview.
    pub matched_scopes: Vec<MatchedScope>,
    pub decision: SplitDecision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchedScope {
    pub ticket: TicketKey,
    pub paths: Vec<String>,
}
