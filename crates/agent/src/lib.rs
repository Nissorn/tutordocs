//! Agent driver orchestration: keyless CLI drivers, jobs and event streams.
//! Ticket 02 owns the real code; the trait contract is PROPOSAL.md's.

pub mod agy;
pub mod catalog;
pub mod claude;
pub(crate) mod cli;
pub mod env;
pub mod logline;
pub mod parse;
pub(crate) mod policy;
pub mod probe;
pub(crate) mod prompt;
pub mod qoder;
pub mod registry;
pub(crate) mod runner;
pub mod types;

// Implementing `AgentDriver` outside this crate, such as a test fake, needs the
// trait's own macro and error type.
pub use anyhow;
pub use async_trait::async_trait;

pub use agy::AgyDriver;
pub use claude::ClaudeDriver;
pub use logline::{fold_events, Folded, LogFold};
pub use qoder::QoderDriver;
pub use types::{
    AgentDriver, AgentEvent, AuthKind, Capabilities, Effort, EventStream, JobHandle, JobSpec,
    ModelId,
};

/// Identifies this crate so the workspace smoke test proves every member runs.
pub fn crate_id() -> &'static str {
    "tutordocs-agent"
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_id_matches_package_name() {
        assert_eq!(super::crate_id(), "tutordocs-agent");
    }
}
