//! Commit fingerprint + ticket assignment.

use crate::error::{Error, Result};
use crate::ticket::TicketKey;
use serde::{Deserialize, Serialize};

/// Confidence level for a proposed commit-to-ticket assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    /// Every path the commit touches falls inside exactly one ticket scope.
    /// Operator review optional.
    HighSingleScope,
    /// Commit paths match multiple scopes; carve has flagged it as
    /// cross-cutting and proposed a split. Operator MUST review.
    LowCrossCutting,
    /// Commit paths match no scope. Operator MUST assign manually or
    /// extend the relevant ticket scope's `paths` field.
    Unassigned,
    /// Operator explicitly set this assignment by hand. Carve will not
    /// override it on regeneration.
    OperatorPinned,
}

/// A `[7-40]`-hex-char git commit SHA. Validated on construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommitSha(String);

impl CommitSha {
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        let len_ok = s.len() >= 7 && s.len() <= 40;
        let hex_ok = s.chars().all(|c| c.is_ascii_hexdigit());
        if !len_ok || !hex_ok {
            return Err(Error::InvalidCommitSha { sha: s });
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CommitSha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One commit, captured at plan-generation time. Immutable input to the
/// plan; never mutated by carve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitFingerprint {
    pub sha: CommitSha,
    /// Full first-line subject. Used to display in plan diffs and PR bodies.
    pub subject: String,
    /// Repo-relative paths touched by this commit.
    pub paths: Vec<String>,
    /// Author email — propagated to all derived commits (cherry-picks +
    /// splits) so credit is preserved.
    pub author: String,
    /// RFC3339 author date — propagated to derived commits so the
    /// chronological ordering in the original branch is faithfully
    /// reflected in the rewritten stack.
    pub author_date: String,
}

/// Carve's proposed assignment of a commit to a single ticket.
///
/// For cross-cutting commits, see [`crate::CrossCuttingCommit`] which
/// carries split decisions; the commit is then represented by multiple
/// [`CommitAssignment`] entries (one per target ticket, each pinned to a
/// subset of the commit's paths).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitAssignment {
    pub sha: CommitSha,
    pub ticket: TicketKey,
    pub confidence: Confidence,
    /// Optional: if this assignment is half of a split, which paths
    /// belong to it. `None` means the entire commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths_subset: Option<Vec<String>>,
    /// Optional: human-readable rationale for the assignment, surfaced in
    /// the plan diff and in `carve verify` output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
}
