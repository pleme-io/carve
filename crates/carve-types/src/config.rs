//! Per-org / per-repo configurability for the JIRA + GitHub systems carve
//! talks to.
//!
//! Layered resolution (highest precedence first):
//!   1. Command-line flags
//!   2. Per-repo `.carve.toml` at the repo root
//!   3. User-global `~/.config/carve/config.toml`
//!   4. Built-in defaults
//!
//! No field is mandatory — every value has a sensible default. Operators
//! only set the ones that diverge from carve's built-ins.

use serde::{Deserialize, Serialize};

/// Top-level config document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarveConfig {
    #[serde(default)]
    pub jira: JiraConfig,
    #[serde(default)]
    pub github: GitHubConfig,
}

/// JIRA-specific knobs. Each carve-managed JIRA system has different
/// custom-field IDs, workflow shape, and team policies; this is where
/// those facts live.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraConfig {
    /// REST custom-field id that holds story points. Differs per JIRA
    /// install. Default is the Jira-Software-Greenhopper field id.
    #[serde(default = "default_story_points_field")]
    pub story_points_field: String,

    /// How many story points represent one operator-day. Our team uses 1
    /// point = 1 day; some teams use fibonacci scales or 1 point = 0.5
    /// days. Carve uses this to derive point values from operator
    /// estimates (`carve jira-sync --days N` → `points = N * 1/scale`).
    #[serde(default = "default_points_per_day")]
    pub points_per_day: f32,

    /// The furthest workflow state carve is permitted to auto-transition
    /// tickets to. If the operator's plan asks for a state past this,
    /// carve stops at `max_auto_transition` and emits a warning telling
    /// the operator to advance the ticket by hand.
    ///
    /// Examples:
    ///  - `"In Review"` (full automation; what akeyless ASM permits today)
    ///  - `"Ready To Work"` (some workflows block automation past this)
    ///  - `null` to disable auto-transition entirely
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_auto_transition: Option<String>,

    /// Optional: pin the transition IDs by name. If unset, carve looks
    /// them up via the JIRA `transitions` endpoint per-issue. Pinning
    /// here saves a round-trip per ticket.
    #[serde(default)]
    pub transition_ids: std::collections::BTreeMap<String, u32>,

    /// Optional: when sync runs, also link the PR back as a remote-link
    /// on the JIRA issue (via `remote_issue_links` API). Default false
    /// since most teams prefer Smart Commits or the GitHub-JIRA
    /// integration to handle linking.
    #[serde(default)]
    pub link_pr_as_remote: bool,

    /// Post a simple ADF comment on each synced ticket noting the PR
    /// number/URL carve associated with it. Useful when the JIRA-GitHub
    /// integration is off or unreliable. Default false to avoid
    /// duplicate-comment noise when integrations *are* on.
    #[serde(default)]
    pub post_pr_link_comment: bool,
}

impl Default for JiraConfig {
    fn default() -> Self {
        Self {
            story_points_field: default_story_points_field(),
            points_per_day: default_points_per_day(),
            max_auto_transition: Some("In Review".into()),
            transition_ids: std::collections::BTreeMap::new(),
            link_pr_as_remote: false,
            post_pr_link_comment: false,
        }
    }
}

impl JiraConfig {
    /// Round-half-up conversion from operator-days estimate to story points,
    /// respecting [`Self::points_per_day`].
    pub fn points_for_days(&self, days: f32) -> f32 {
        (days * self.points_per_day * 100.0).round() / 100.0
    }

    /// Whether carve is permitted to transition a ticket to the given
    /// workflow state. Always allows transitions to `max_auto_transition`
    /// itself; refuses anything claimed to be "after" it via [`stage_rank`].
    pub fn may_transition_to(&self, target_status: &str) -> bool {
        match &self.max_auto_transition {
            None => false,
            Some(max) => stage_rank(target_status) <= stage_rank(max),
        }
    }
}

/// Coarse rank of common Jira workflow stages. Higher rank = later. Used
/// to reason about "past X" comparisons without round-tripping to Jira.
/// Unknown names rank at 1000 — i.e. assumed past everything — so carve
/// refuses to auto-transition into states it can't reason about.
pub fn stage_rank(status: &str) -> i32 {
    match status.trim().to_ascii_lowercase().as_str() {
        "open" | "to do" | "todo" | "backlog" | "ready" => 10,
        "additional info is needed" => 12,
        "ready to work" | "ready for work" | "next" => 20,
        "in progress" | "in dev" => 30,
        "in review" | "code review" | "review" => 40,
        "customer validation" | "validation" => 50,
        "work complete" => 60,
        "done" | "closed" | "resolved" => 70,
        _ => 1000,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubConfig {
    /// Default reviewer login (or comma list) for `carve execute`-created
    /// PRs. Falls back to empty (no reviewer requested).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reviewer: Option<String>,
    /// If true, mark non-placeholder PRs ready-for-review on creation
    /// (default). If false, they're opened as drafts the operator
    /// promotes manually.
    #[serde(default = "default_true")]
    pub open_ready_for_review: bool,
}

fn default_story_points_field() -> String {
    "customfield_10016".to_string()
}

fn default_points_per_day() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_per_day_default_one_to_one() {
        let c = JiraConfig::default();
        assert_eq!(c.points_for_days(1.0), 1.0);
        assert_eq!(c.points_for_days(2.0), 2.0);
        assert_eq!(c.points_for_days(0.5), 0.5);
    }

    #[test]
    fn points_per_day_half_day_scale() {
        let c = JiraConfig {
            points_per_day: 2.0, // 2 points per day → 0.5 days per point
            ..Default::default()
        };
        assert_eq!(c.points_for_days(1.0), 2.0);
        assert_eq!(c.points_for_days(0.5), 1.0);
    }

    #[test]
    fn may_transition_respects_max() {
        let strict = JiraConfig {
            max_auto_transition: Some("Ready To Work".into()),
            ..Default::default()
        };
        assert!(strict.may_transition_to("Ready To Work"));
        assert!(strict.may_transition_to("To Do"));
        assert!(!strict.may_transition_to("In Review"));
        assert!(!strict.may_transition_to("Done"));

        let default = JiraConfig::default();
        assert!(default.may_transition_to("In Review"));
        assert!(!default.may_transition_to("Done"));
    }

    #[test]
    fn unknown_states_are_refused() {
        let c = JiraConfig::default();
        // "Mystery" not in stage_rank → ranks 1000 → above max → refused.
        assert!(!c.may_transition_to("Mystery"));
    }
}
