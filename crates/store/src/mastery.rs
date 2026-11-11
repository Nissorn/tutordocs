//! Mastery: an append-only event log plus a repetition state derived from it.
//!
//! The `mastery_events` shape is translated from DeepTutor `learning/storage.py`
//! (Apache-2.0, (c) 2025 Data Intelligence Lab, HKU), pinned commit 60fb3bdf.
//! `RepetitionState` fields are translated from `learning/models.py:202-223`.
//!
//! Truth rule: the log is the only stored mastery truth. There is no mutable
//! state snapshot table; `derive_repetition_state` is a pure fold over the log,
//! so reopening the database recomputes the identical state. The full FSRS-like
//! stability/retrievability transition (`scheduler.py`, port-shapes item 2) is
//! ticket 10's scope; the fold here is the deterministic ticket-04 seam it will
//! deepen, using the ported MEMORY interval sequence and result mapping.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{Result, StoreError};

/// Days per review round for the memory knowledge type
/// (`scheduler.py:17-22`, `INTERVAL_SEQUENCES[MEMORY]`).
const MEMORY_INTERVALS: [f64; 7] = [0.0, 1.0, 3.0, 7.0, 14.0, 30.0, 60.0];
/// Seconds in a day; the normal scheduling unit (`scheduler.py:174`).
const DAY_SECONDS: f64 = 86_400.0;
/// Default desired retention (`scheduler.py:57`).
const DEFAULT_DESIRED_RETENTION: f64 = 0.9;

/// A new event to append. `id` is assigned by SQLite (autoincrement), so the
/// caller never supplies it; ordering is by `id`, not by timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewMasteryEvent {
    /// The objective/path this event belongs to.
    pub path_id: String,
    /// Caller-managed revision (the engine owns CAS; the store just records it).
    pub revision: i64,
    /// Domain event name, e.g. `mastery.assessed`. Must be non-empty.
    pub event_type: String,
    /// JSON payload; defaults to `{}` when empty.
    pub payload_json: String,
    /// Originating session, may be empty.
    pub session_id: String,
    /// Originating turn, may be empty.
    pub turn_id: String,
    /// Unix seconds.
    pub created_at: f64,
}

impl NewMasteryEvent {
    /// An event with empty session/turn and the given payload.
    pub fn new(
        path_id: impl Into<String>,
        revision: i64,
        event_type: impl Into<String>,
        created_at: f64,
    ) -> Self {
        Self {
            path_id: path_id.into(),
            revision,
            event_type: event_type.into(),
            payload_json: "{}".to_string(),
            session_id: String::new(),
            turn_id: String::new(),
            created_at,
        }
    }
}

/// A stored event, including its monotone `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MasteryEvent {
    /// Autoincrement primary key; the append order.
    pub id: i64,
    /// The objective/path id.
    pub path_id: String,
    /// Revision recorded with the event.
    pub revision: i64,
    /// Domain event name.
    pub event_type: String,
    /// JSON payload.
    pub payload_json: String,
    /// Originating session.
    pub session_id: String,
    /// Originating turn.
    pub turn_id: String,
    /// Unix seconds.
    pub created_at: f64,
}

/// Append one event. This is the only write path; nothing updates or deletes an
/// event in place. Returns the assigned `id`. An empty `event_type` is rejected,
/// mirroring `LearningTransaction.record` (`storage.py:140-151`).
pub(crate) fn append(conn: &Connection, e: &NewMasteryEvent) -> Result<i64> {
    if e.event_type.trim().is_empty() {
        return Err(StoreError::InvalidEvent(
            "event_type must not be empty".to_string(),
        ));
    }
    let payload = if e.payload_json.trim().is_empty() {
        "{}"
    } else {
        &e.payload_json
    };
    conn.execute(
        "INSERT INTO mastery_events
            (path_id, revision, event_type, payload_json, session_id, turn_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            e.path_id,
            e.revision,
            e.event_type,
            payload,
            e.session_id,
            e.turn_id,
            e.created_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Read a path's events in append order (`id`), never by timestamp.
pub(crate) fn list_for_path(conn: &Connection, path_id: &str) -> Result<Vec<MasteryEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, path_id, revision, event_type, payload_json, session_id, turn_id, created_at
         FROM mastery_events WHERE path_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![path_id], |row| {
        Ok(MasteryEvent {
            id: row.get(0)?,
            path_id: row.get(1)?,
            revision: row.get(2)?,
            event_type: row.get(3)?,
            payload_json: row.get(4)?,
            session_id: row.get(5)?,
            turn_id: row.get(6)?,
            created_at: row.get(7)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// The review schedule, derived from the log. Fields mirror DeepTutor
/// `RepetitionState` (`models.py:202-223`). The adaptive-SRS fields
/// (`difficulty`, `stability`, `retrievability`) carry their ported defaults
/// here; ticket 10's full transition fills them. Nothing stores this struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepetitionState {
    /// Index into the interval sequence.
    pub interval_index: i64,
    /// Consecutive correct reviews.
    pub consecutive_correct: i64,
    /// Consecutive wrong reviews.
    pub consecutive_wrong: i64,
    /// Unix seconds when the next review is due.
    pub next_review_at: f64,
    /// FSRS difficulty; default until ticket 10.
    pub difficulty: f64,
    /// FSRS stability; 0 means never hydrated.
    pub stability: f64,
    /// FSRS retrievability.
    pub retrievability: f64,
    /// Target retention.
    pub desired_retention: f64,
    /// Total reviews folded.
    pub review_count: i64,
    /// Total lapses folded.
    pub lapse_count: i64,
    /// Last review timestamp, if any.
    pub last_review_at: Option<f64>,
    /// Timestamp of the review that set `next_review_at`.
    pub last_scheduled_at: Option<f64>,
    /// Whether the last schedule followed a failure.
    pub scheduled_after_failure: bool,
}

impl Default for RepetitionState {
    fn default() -> Self {
        Self {
            interval_index: 0,
            consecutive_correct: 0,
            consecutive_wrong: 0,
            next_review_at: 0.0,
            difficulty: 0.3,
            stability: 0.0,
            retrievability: 1.0,
            desired_retention: DEFAULT_DESIRED_RETENTION,
            review_count: 0,
            lapse_count: 0,
            last_review_at: None,
            last_scheduled_at: None,
            scheduled_after_failure: false,
        }
    }
}

#[derive(Deserialize)]
struct OutcomePayload {
    #[serde(default)]
    result: Option<String>,
}

/// Recency weights for the most recent attempts, oldest to newest
/// (`learning/mastery.py:17`).
const RECENCY_WEIGHTS: [f64; 5] = [0.5, 0.7, 0.85, 0.95, 1.0];

/// Mastery cannot exceed these until enough attempts accumulate, keyed by the
/// number of recent attempts (`learning/mastery.py:21`).
fn confidence_cap(recent: usize) -> f64 {
    match recent {
        1 => 0.5,
        2 => 0.8,
        _ => 1.0,
    }
}

/// A 0..1 mastery score from attempt outcomes in chronological order, each a
/// correctness in 0..1 (partial credit allowed). Ported from `compute_mastery`
/// (`learning/mastery.py:24-37`): a recency-weighted mean over the last five
/// attempts, capped while there are fewer than three.
pub fn compute_mastery(outcomes: &[f64]) -> f64 {
    if outcomes.is_empty() {
        return 0.0;
    }
    let recent = &outcomes[outcomes.len().saturating_sub(RECENCY_WEIGHTS.len())..];
    let weights = &RECENCY_WEIGHTS[RECENCY_WEIGHTS.len() - recent.len()..];
    let weighted: f64 = recent
        .iter()
        .zip(weights)
        .map(|(c, w)| w * c.clamp(0.0, 1.0))
        .sum();
    let score = weighted / weights.iter().sum::<f64>();
    score.min(confidence_cap(recent.len()))
}

/// The correctness a graded result counts for: partial is half credit
/// (`LearningService.calculate_mastery`, `learning/service.py:522-529`).
/// Anything else, `ungraded` included, is not a graded attempt.
fn graded_value(result: &str) -> Option<f64> {
    match result {
        "correct" => Some(1.0),
        "partial" => Some(0.5),
        "incorrect" => Some(0.0),
        _ => None,
    }
}

/// An objective's mastery, projected from its log: every graded result in
/// append order, fed to [`compute_mastery`]. Nothing stores the level.
pub fn mastery_level(events: &[MasteryEvent]) -> f64 {
    let outcomes: Vec<f64> = events
        .iter()
        .filter_map(|e| serde_json::from_str::<OutcomePayload>(&e.payload_json).ok())
        .filter_map(|p| p.result.as_deref().and_then(graded_value))
        .collect();
    compute_mastery(&outcomes)
}

/// Pure fold over a path's events in append order. Only events whose payload
/// carries a review `result` (correct/partial/incorrect) advance the schedule;
/// audit events like `path.created` leave it untouched. Deterministic: the same
/// log always yields the same state, which is what makes reopen durable.
///
/// This is the ticket-04 derivation seam. Ticket 10 replaces the interval-step
/// with the full ported FSRS transition (port-shapes item 2) without changing
/// the log shape or this function's contract.
pub fn derive_repetition_state(events: &[MasteryEvent], now: f64) -> RepetitionState {
    let mut state = RepetitionState::default();
    let last_round = (MEMORY_INTERVALS.len() - 1) as i64;
    for event in events {
        let payload: OutcomePayload = match serde_json::from_str(&event.payload_json) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let result = match payload.result.as_deref() {
            Some(r) => r,
            None => continue,
        };
        let correct = result == "correct";
        let moment = event
            .created_at
            .max(state.last_review_at.unwrap_or(event.created_at));
        state.review_count += 1;
        state.last_review_at = Some(moment);
        if correct {
            state.consecutive_correct += 1;
            state.consecutive_wrong = 0;
            state.interval_index = (state.interval_index + 1).min(last_round);
            state.scheduled_after_failure = false;
        } else {
            state.consecutive_wrong += 1;
            state.consecutive_correct = 0;
            state.lapse_count += 1;
            state.interval_index = 0;
            state.scheduled_after_failure = true;
        }
        let interval_days = MEMORY_INTERVALS[state.interval_index as usize];
        state.next_review_at = moment + interval_days * DAY_SECONDS;
        state.last_scheduled_at = Some(moment);
    }
    if state.review_count == 0 {
        // Never reviewed: due now, the initial-schedule anchor.
        state.next_review_at = now;
        state.last_scheduled_at = Some(now);
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(id: i64, result: &str, at: f64) -> MasteryEvent {
        MasteryEvent {
            id,
            path_id: "p".into(),
            revision: 1,
            event_type: "mastery.assessed".into(),
            payload_json: format!("{{\"result\":\"{result}\"}}"),
            session_id: String::new(),
            turn_id: String::new(),
            created_at: at,
        }
    }

    #[test]
    fn empty_log_is_due_now() {
        let state = derive_repetition_state(&[], 1000.0);
        assert_eq!(state.next_review_at, 1000.0);
        assert_eq!(state.review_count, 0);
    }

    #[test]
    fn correct_streak_advances_interval() {
        let events = vec![review(1, "correct", 0.0), review(2, "correct", DAY_SECONDS)];
        let state = derive_repetition_state(&events, DAY_SECONDS);
        assert_eq!(state.review_count, 2);
        assert_eq!(state.consecutive_correct, 2);
        assert_eq!(state.interval_index, 2);
        assert_eq!(state.next_review_at, DAY_SECONDS + 3.0 * DAY_SECONDS);
    }

    #[test]
    fn failure_resets_interval_and_counts_lapse() {
        let events = vec![
            review(1, "correct", 0.0),
            review(2, "incorrect", DAY_SECONDS),
        ];
        let state = derive_repetition_state(&events, DAY_SECONDS);
        assert_eq!(state.lapse_count, 1);
        assert_eq!(state.interval_index, 0);
        assert!(state.scheduled_after_failure);
        assert_eq!(state.next_review_at, DAY_SECONDS);
    }

    #[test]
    fn audit_events_do_not_advance_schedule() {
        let mut audit = review(1, "correct", 0.0);
        audit.event_type = "path.created".into();
        audit.payload_json = "{}".into();
        let state = derive_repetition_state(&[audit], 5.0);
        assert_eq!(state.review_count, 0);
    }
}
