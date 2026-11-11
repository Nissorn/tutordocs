//! Progress per course objective, projected from the mastery event log. The
//! log is the only stored truth: nothing here writes a level or a schedule, so
//! reopening the library recomputes the same progress.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tutordocs_store::{mastery_level, Library};

use crate::quiz::QuizError;

/// The event a final grade appends (`storage.py:256` names it).
pub(crate) const ASSESSED: &str = "mastery.assessed";

/// At or above this level an objective that is not due needs no review. A
/// third correct answer in a row is the first that can reach it, because
/// `compute_mastery` caps two answers at 0.8.
const MASTERED: f64 = 0.8;

/// One objective's progress, as the sidebar shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectiveProgress {
    /// The objective's text from `course.json`.
    pub objective: String,
    /// 0..1, from `compute_mastery` over every graded answer.
    pub mastery: f64,
    /// How many answers were graded final.
    pub attempts: usize,
    /// Unix seconds when a review is due. `None` until the first grade.
    pub next_review_at: Option<f64>,
    /// Whether that time has come.
    pub due: bool,
}

/// The log path one objective's events go to.
pub(crate) fn objective_key(course_id: &str, objective: &str) -> String {
    format!("{course_id}/{objective}")
}

/// Every objective of the library's course, in outline order.
pub fn course_progress(root: &Path, now: f64) -> Result<Vec<ObjectiveProgress>, QuizError> {
    let library = Library::open(root)?;
    let course = library.course()?;
    course
        .objectives
        .iter()
        .map(|objective| {
            let key = objective_key(&course.id, objective);
            let events = library.mastery_events(&key)?;
            let attempts = events.iter().filter(|e| e.event_type == ASSESSED).count();
            let next_review_at = if attempts == 0 {
                None
            } else {
                Some(library.repetition_state(&key, now)?.next_review_at)
            };
            Ok(ObjectiveProgress {
                objective: objective.clone(),
                mastery: mastery_level(&events),
                attempts,
                next_review_at,
                due: next_review_at.is_some_and(|at| at <= now),
            })
        })
        .collect()
}

/// The objectives to review next: those already assessed that are due or not
/// yet mastered, due ones first, then weakest first.
pub(crate) fn review_first(progress: &[ObjectiveProgress]) -> Vec<&ObjectiveProgress> {
    let mut first: Vec<&ObjectiveProgress> = progress
        .iter()
        .filter(|p| p.attempts > 0 && (p.due || p.mastery < MASTERED))
        .collect();
    first.sort_by(|a, b| b.due.cmp(&a.due).then(a.mastery.total_cmp(&b.mastery)));
    first
}

/// Now, in Unix seconds.
pub fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}
