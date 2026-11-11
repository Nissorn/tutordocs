//! Learning engine: mastery events, repetition state and grading orchestration.
//! Ticket 12 owns URL ingestion here; ticket 07 owns outline-first course
//! generation (`outline`, `chapter`); the later engine tickets add the rest.

pub mod agents;
mod ask;
mod chapter;
mod confine;
mod error;
mod generation;
mod grading;
mod ingest;
mod joblog;
mod outline;
mod progress;
mod quiz;
mod rewrite_instruction;
mod template;
mod time;
mod url;

pub use agents::{
    blocking as agent_blocking, AgentChoice, AgentOverrides, AgentsState, DriverTuning,
};
pub use ask::{ask_about, list_answers};
pub use chapter::{
    approved_chapter_title, chapter_context, chapter_prompt, chapter_title, ready_chapter_doc,
    revise_section, ApproveGenerate, ChapterContext, ChapterJobs, ChapterQueue, ChapterQueueTask,
    GenerationState, OutlineSummary,
};
pub use error::IngestError;
pub use generation::{GenerationError, Progress};
pub use grading::{grade_answers, Answer, GradedAnswer, Outcome, GRADE_CONFIDENCE_THRESHOLD};
pub use ingest::ingest_url;
pub use joblog::{
    job_scope, EmitFn, JobKind, JobLog, JobOutcome, JobRecord, JobRegistry, JobStart, JobStatus,
    LineKind, LogLine,
};
pub use outline::{generate_outline, revise_outline};
pub use progress::{course_progress, unix_now, ObjectiveProgress};
pub use quiz::{generate_quiz, Question, Quiz, QuizError, MAX_QUESTIONS};
pub use rewrite_instruction::{RewriteInstruction, MAX_INSTRUCTION_CHARS};
pub use template::template_instructions;
pub use time::rfc3339_utc;
pub use tutordocs_store::{
    list_course_templates, ChapterAnswer, CourseSetup, OutlineVariant, Template, VariantMeta,
};

/// Identifies this crate so the workspace smoke test proves every member runs.
pub fn crate_id() -> &'static str {
    "tutordocs-engine"
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_id_matches_package_name() {
        assert_eq!(super::crate_id(), "tutordocs-engine");
    }
}
