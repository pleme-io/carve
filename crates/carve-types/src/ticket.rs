//! Ticket scope — what makes a commit "belong" to a given JIRA ticket.
//!
//! A [`TicketScope`] is the operator's declaration of what files / paths are
//! in-scope for a given JIRA key. The plan generator uses these scopes to
//! propose commit-to-ticket assignments by intersecting commit paths with
//! scope globs. The operator can hand-edit assignments after generation;
//! scopes are the *hint*, not the *verdict*.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// A JIRA issue key — uppercase project prefix + dash + integer.
///
/// Examples: `ASM-18003`, `PROJ-42`. Validated on construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TicketKey(String);

impl TicketKey {
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        let bytes = s.as_bytes();
        let dash_pos = bytes.iter().position(|&b| b == b'-').ok_or_else(|| {
            Error::InvalidTicketKey {
                key: s.clone(),
                reason: "missing dash separator",
            }
        })?;
        if dash_pos == 0 || dash_pos == bytes.len() - 1 {
            return Err(Error::InvalidTicketKey {
                key: s,
                reason: "dash at start or end",
            });
        }
        let (project, num) = s.split_at(dash_pos);
        if !project.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            return Err(Error::InvalidTicketKey {
                key: s,
                reason: "project prefix not uppercase",
            });
        }
        let num = &num[1..];
        if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
            return Err(Error::InvalidTicketKey {
                key: s,
                reason: "issue number must be digits",
            });
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TicketKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lifecycle state of a ticket as known to carve.
///
/// Carve only tracks the subset relevant to PR delivery; full JIRA state
/// machine is intentionally not mirrored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TicketStatus {
    /// No carve PR yet; not started.
    ReadyToWork,
    /// Carve PR is open and awaiting review.
    InReview,
    /// PR merged; ticket done.
    Done,
    /// Operator declares this ticket has no repo-side scope (e.g. docs
    /// tracked elsewhere); a placeholder PR exists but no content.
    Placeholder,
}

/// Operator-declared scope for a single ticket.
///
/// `paths` is the *positive* set: any commit touching at least one path
/// matching a glob in `paths` is a candidate for this ticket. `exclude` is
/// the *negative* override: paths that would otherwise match `paths` but are
/// claimed by a more-specific ticket. Cross-cutting commits — touching paths
/// in multiple tickets' positive sets — are surfaced to the operator for
/// explicit split decisions; carve never silently picks one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketScope {
    pub key: TicketKey,
    pub summary: String,
    /// Glob patterns relative to repo root. A commit qualifies for this
    /// ticket if it touches at least one path matching one of these.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Glob patterns to exclude — even if a commit's path matches `paths`,
    /// if it also matches `exclude` it's NOT assigned here.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// If true, this ticket has no repo-side scope — generate an empty
    /// placeholder PR (single `--allow-empty` commit). Plan execution
    /// refuses to assign any commits to a placeholder ticket.
    #[serde(default)]
    pub placeholder: bool,
    /// Order in the stack — lower = closer to master. Operator-controlled.
    pub stack_order: u32,
    /// Optional: story points to set on the JIRA ticket during sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub story_points: Option<f32>,
    /// Optional: target JIRA transition name (e.g. "In Review") for sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_key_valid() {
        assert!(TicketKey::new("ASM-18003").is_ok());
        assert!(TicketKey::new("PROJ-1").is_ok());
        assert!(TicketKey::new("FOO_BAR-42").is_ok());
    }

    #[test]
    fn ticket_key_invalid() {
        assert!(TicketKey::new("asm-18003").is_err()); // lowercase
        assert!(TicketKey::new("ASM18003").is_err()); // no dash
        assert!(TicketKey::new("ASM-").is_err()); // dash at end
        assert!(TicketKey::new("-18003").is_err()); // dash at start
        assert!(TicketKey::new("ASM-abc").is_err()); // non-digit number
    }
}
