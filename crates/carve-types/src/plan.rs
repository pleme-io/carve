//! The [`Plan`] — top-level YAML artifact for carve.

use crate::commit::{CommitAssignment, CommitFingerprint};
use crate::cross_cutting::CrossCuttingCommit;
use crate::error::{Error, Result};
use crate::stack::StackTopology;
use crate::ticket::TicketScope;
use serde::{Deserialize, Serialize};

/// Identification of the monolithic input branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceBranch {
    /// The branch carve is carving (e.g. `ASM-18003-dbk-asia-southeast1-staging`).
    pub name: String,
    /// The branch carve diverges from — every stack node ultimately roots
    /// on this. Usually `master` or `main`.
    pub master_branch: String,
    /// The tip SHA of the source branch at plan-generation time.
    pub tip: String,
    /// The merge-base of `name` and `master_branch` at plan-generation time.
    /// Defines the commit range carve considers.
    pub merge_base: String,
}

/// Generator + provenance metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanMeta {
    /// carve binary version that produced this plan.
    pub carve_version: String,
    /// RFC3339 timestamp.
    pub generated_at: String,
    /// The JIRA epic that fans out into stack sub-tickets.
    pub jira_epic: String,
    /// Operator identifier (git user.email) — recorded so it's clear who
    /// owns this plan iteration.
    pub operator: String,
}

/// The full plan. Round-trip-serializable as `plan.yaml`; **all fields**
/// are operator-editable between `carve plan` and `carve execute`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub meta: PlanMeta,
    pub source: SourceBranch,
    pub tickets: Vec<TicketScope>,
    /// The full commit range from `source.merge_base..source.tip` captured
    /// at plan-generation time. Operators don't usually edit this; it's
    /// the immutable input. Mutations to this list cause `carve verify`
    /// to refuse — re-run `carve plan` first.
    pub commits: Vec<CommitFingerprint>,
    /// Commit-to-ticket assignments, one per commit (or two+ if a commit
    /// is split, in which case there's also a `cross_cutting` entry).
    pub assignments: Vec<CommitAssignment>,
    pub cross_cutting: Vec<CrossCuttingCommit>,
    pub stack: StackTopology,
}

impl Plan {
    pub fn to_yaml(&self) -> Result<String> {
        serde_yaml::to_string(self).map_err(Error::from)
    }

    pub fn from_yaml(s: &str) -> Result<Self> {
        serde_yaml::from_str(s).map_err(Error::from)
    }

    /// Cheap structural invariants — `carve verify` runs deeper checks
    /// (e.g. cross-cutting halves cover all original paths exactly once).
    pub fn check_basic_invariants(&self) -> Result<()> {
        // Every assignment's ticket must exist in self.tickets.
        let ticket_keys: std::collections::HashSet<_> =
            self.tickets.iter().map(|t| t.key.as_str()).collect();
        for a in &self.assignments {
            if !ticket_keys.contains(a.ticket.as_str()) {
                return Err(Error::PlanInvariant(format!(
                    "assignment for commit {} references unknown ticket {}",
                    a.sha, a.ticket
                )));
            }
        }
        // Every stack node's ticket must exist in self.tickets.
        for n in &self.stack.nodes {
            if !ticket_keys.contains(n.ticket.as_str()) {
                return Err(Error::PlanInvariant(format!(
                    "stack node {} references unknown ticket {}",
                    n.branch, n.ticket
                )));
            }
        }
        // Stack nodes must be in non-decreasing stack_order.
        let mut last_order: i64 = -1;
        for n in &self.stack.nodes {
            let t = self
                .tickets
                .iter()
                .find(|t| t.key == n.ticket)
                .expect("validated above");
            let order = t.stack_order as i64;
            if order < last_order {
                return Err(Error::PlanInvariant(format!(
                    "stack node {} (order {}) is out-of-order vs previous (order {})",
                    n.branch, order, last_order
                )));
            }
            last_order = order;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_plan() -> Plan {
        Plan {
            meta: PlanMeta {
                carve_version: "0.1.0".into(),
                generated_at: "2026-05-20T00:00:00Z".into(),
                jira_epic: "ASM-18003".into(),
                operator: "luis.d@akeyless.io".into(),
            },
            source: SourceBranch {
                name: "demo".into(),
                master_branch: "master".into(),
                tip: "abc1234".into(),
                merge_base: "def5678".into(),
            },
            tickets: vec![],
            commits: vec![],
            assignments: vec![],
            cross_cutting: vec![],
            stack: StackTopology {
                root: "master".into(),
                nodes: vec![],
            },
        }
    }

    #[test]
    fn round_trips_through_yaml() {
        let p = minimal_plan();
        let yaml = p.to_yaml().unwrap();
        let back = Plan::from_yaml(&yaml).unwrap();
        assert_eq!(back.meta.jira_epic, "ASM-18003");
    }

    #[test]
    fn empty_plan_passes_basic_invariants() {
        let p = minimal_plan();
        assert!(p.check_basic_invariants().is_ok());
    }
}
