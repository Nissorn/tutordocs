//! Why an ingestion did not produce a source. The UI shows these messages.

use tutordocs_store::StoreError;

/// Everything `ingest_url` can fail with. Every variant leaves `sources/`
/// exactly as it was.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    /// The text is not an `http` or `https` address the agent should fetch.
    #[error("cannot ingest {url:?}: {reason}")]
    InvalidUrl {
        /// What the user submitted.
        url: String,
        /// Which rule it broke.
        reason: &'static str,
    },

    /// The library folder or its sources could not be read or written.
    #[error(transparent)]
    Store(#[from] StoreError),

    /// The agent driver could not start the job.
    #[error("could not start the agent: {0}")]
    Driver(String),

    /// The agent job reported an error, such as a timeout or a failed fetch.
    #[error("the agent could not fetch the page: {0}")]
    JobFailed(String),

    /// The job ended cleanly but saved no usable markdown. Carries what the
    /// agent said, which is usually why (not found, paywall, blocked).
    #[error("the agent finished without saving the page: {0}")]
    NothingFetched(String),
}
