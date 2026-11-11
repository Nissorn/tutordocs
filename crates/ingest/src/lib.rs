//! Ingestion: PDF text with positions, markdown and URL capture into sources/.
//! Compilable stub from ticket 01; the ingestion tickets own the real code.

pub mod fixture;
pub mod pdf;

/// Identifies this crate so the workspace smoke test proves every member runs.
pub fn crate_id() -> &'static str {
    "tutordocs-ingest"
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_id_matches_package_name() {
        assert_eq!(super::crate_id(), "tutordocs-ingest");
    }
}
