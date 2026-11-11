//! Storage: the markdown folder as content truth, SQLite for annotations and
//! progress. Neither is derived from the other (PROPOSAL truth rule).
//!
//! The public seam is [`Library`]: create/open a folder, list its course and
//! chapters (placeholder-aware), and write/read annotations and mastery events.
//! Durability is proven by closing and reopening the on-disk database.
//!
//! Decision 10: no FTS, no embeddings, no vector table, no sync machinery.
//!
//! Portions translated from DeepTutor (Apache-2.0, (c) 2025 Data Intelligence
//! Lab, HKU), pinned commit 60fb3bdf: the annotation field shape and W3C
//! selectors (`reading/models.py`), the `mastery_events` log (`learning/
//! storage.py`) and `RepetitionState` (`learning/models.py`).

mod annotation;
mod answer;
mod db;
mod error;
mod library;
mod mastery;
mod setup;
mod shelf;
mod source;

pub use annotation::{Annotation, AnnotationKind, TextSelector, DEFAULT_COLOR};
pub use answer::{ChapterAnswer, NewAnswer};
pub use error::{Result, StoreError};
pub use library::{
    Chapter, ChapterBody, ChapterDoc, ChapterRef, ChapterSummary, Course, Library, Outline,
    OutlineChapter, OutlineStatus, OutlineVariant, VariantMeta,
};
pub use mastery::{
    compute_mastery, derive_repetition_state, mastery_level, MasteryEvent, NewMasteryEvent,
    RepetitionState,
};
pub use setup::{list_course_templates, template_instructions_for_id, CourseSetup, Template};
pub use shelf::{
    default_library_root, is_plain_component, resolve_course_dir, rfc3339_utc, CourseSummary,
    OutlineCourseSummary, Shelf, TrashEntry,
};
pub use source::{
    CourseSource, ImportResult, ImportStatus, Source, SourceDto, SourceKind, SourceOrigin,
};

/// Identifies this crate so the workspace smoke test proves every member runs.
pub fn crate_id() -> &'static str {
    "tutordocs-store"
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_id_matches_package_name() {
        assert_eq!(super::crate_id(), "tutordocs-store");
    }
}
