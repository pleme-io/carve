//! carve-types — typed data model for the carve stacked-PR primitive.
//!
//! The crate is **plan-centric**: every operation reads or writes a [`Plan`]
//! artifact, which is the operator-editable YAML document that maps commits
//! on a monolithic branch onto ticket-aligned stacked PRs. The CLI binary
//! never invents structure that isn't representable here; the type system is
//! the source of truth.
//!
//! ## Lifecycle
//!
//! ```text
//!   carve plan     →  produces  plan.yaml  (Plan)
//!   operator edits →  refines   plan.yaml
//!   carve verify   →  consumes  plan.yaml  (dry-run)
//!   carve execute  →  consumes  plan.yaml  (applies)
//!   carve jira-sync→  consumes  plan.yaml  (updates JIRA)
//!   carve diagram  →  consumes  plan.yaml  (regenerates PR bodies)
//! ```

pub mod attestation;
pub mod commit;
pub mod cross_cutting;
pub mod error;
pub mod plan;
pub mod stack;
pub mod ticket;

pub use attestation::{BackupTag, BlakeHash};
pub use commit::{CommitAssignment, CommitFingerprint, Confidence};
pub use cross_cutting::{CrossCuttingCommit, SplitDecision, SplitHalf};
pub use error::{Error, Result};
pub use plan::{Plan, PlanMeta, SourceBranch};
pub use stack::{StackNode, StackTopology};
pub use ticket::{TicketKey, TicketScope, TicketStatus};
