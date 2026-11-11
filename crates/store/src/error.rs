//! Errors surfaced by the store. Recoverable domain errors, never panics.

use std::path::PathBuf;

/// Everything the store can fail with. The UI and the agent both read these
/// messages, so they stay user-facing rather than internal jargon.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Filesystem trouble while reading or writing the library folder.
    #[error("library filesystem error at {path}: {source}")]
    Io {
        /// The path the operation touched.
        path: PathBuf,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// The folder is not a library (missing `course.json` or bad layout).
    #[error("not a TutorDocs library at {0}: missing or unreadable course.json")]
    NotALibrary(PathBuf),

    /// `create` refused to clobber an existing `course.json`.
    #[error("a course.json already exists at {0}; open it instead of creating")]
    AlreadyExists(PathBuf),

    /// `course.json` could not be parsed into the outline shape.
    #[error("invalid course.json: {0}")]
    InvalidCourseJson(String),

    /// An event failed validation before it could be appended to the log.
    #[error("invalid mastery event: {0}")]
    InvalidEvent(String),

    /// A source with this name is already in `sources/`; nothing was replaced.
    #[error("a source named {0} already exists; nothing was replaced")]
    SourceExists(String),

    /// No source with this name is in `sources/`.
    #[error("no source named {0} in sources/")]
    SourceNotFound(String),

    /// The name is not a bare markdown file name inside `sources/`.
    #[error("invalid source name {0:?}: expected a bare .md file name")]
    InvalidSourceName(String),

    /// A chapter id is not one plain directory name under `chapters/`.
    #[error("invalid chapter id {0:?}: expected one plain directory name")]
    InvalidChapterId(String),

    /// A course id is not one plain directory name under `courses/`.
    #[error("invalid course id {0:?}: expected one plain directory name")]
    InvalidCourseId(String),

    /// No course folder has this id.
    #[error("no course {0:?} in this library")]
    CourseNotFound(String),

    /// The caller asked for something the shelf will not do.
    #[error("{0}")]
    Rejected(String),

    /// A one-course root cannot be moved into `courses/` without merging.
    #[error("{0}")]
    MigrationConflict(String),

    /// SQLite reported an error.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// JSON (de)serialization failed for a stored payload.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, StoreError>;
