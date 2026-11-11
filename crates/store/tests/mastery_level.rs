//! `compute_mastery` and the per-objective level it projects from the event
//! log. Every expected value is worked by hand from the recency weights
//! (0.5, 0.7, 0.85, 0.95, 1.0) and the cap {1: 0.5, 2: 0.8}, never by calling
//! the function under test.

use tutordocs_store::{compute_mastery, mastery_level, MasteryEvent};

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn no_attempts_is_zero_mastery() {
    close(compute_mastery(&[]), 0.0);
}

#[test]
fn one_lucky_answer_is_capped_at_half() {
    close(compute_mastery(&[1.0]), 0.5);
}

#[test]
fn two_answers_are_capped_at_point_eight() {
    close(compute_mastery(&[1.0, 1.0]), 0.8);
    // (0 * 0.95 + 1 * 1.0) / 1.95, under the cap.
    close(compute_mastery(&[0.0, 1.0]), 1.0 / 1.95);
}

#[test]
fn newer_attempts_weigh_more_than_older_ones() {
    // weights 0.85, 0.95, 1.0: (0 + 0 + 1) / 2.8
    close(compute_mastery(&[0.0, 0.0, 1.0]), 1.0 / 2.8);
    // weights 0.85, 0.95, 1.0: (1 + 0 + 0) * 0.85 / 2.8
    close(compute_mastery(&[1.0, 0.0, 0.0]), 0.85 / 2.8);
    // weights 0.7, 0.85, 0.95, 1.0: (0.7 + 0 + 0.95 + 1.0) / 3.5
    close(compute_mastery(&[1.0, 0.0, 1.0, 1.0]), 2.65 / 3.5);
}

#[test]
fn only_the_last_five_attempts_count() {
    // The leading 0.0 falls out. Last five 0.5, 0, 1, 1, 1:
    // (0.25 + 0 + 0.85 + 0.95 + 1.0) / 4.0
    close(compute_mastery(&[0.0, 0.5, 0.0, 1.0, 1.0, 1.0]), 3.05 / 4.0);
}

#[test]
fn values_outside_zero_to_one_are_clamped() {
    // (0 * 0.85 + 1 * 0.95 + 1 * 1.0) / 2.8
    close(compute_mastery(&[-3.0, 7.0, 1.0]), 1.95 / 2.8);
}

fn event(id: i64, event_type: &str, payload: &str) -> MasteryEvent {
    MasteryEvent {
        id,
        path_id: "obj".into(),
        revision: 1,
        event_type: event_type.into(),
        payload_json: payload.into(),
        session_id: String::new(),
        turn_id: String::new(),
        created_at: id as f64,
    }
}

#[test]
fn the_level_reads_graded_results_in_log_order_and_skips_the_rest() {
    let log = [
        event(1, "path.created", "{}"),
        event(2, "mastery.assessed", r#"{"result":"incorrect"}"#),
        event(3, "mastery.assessed", r#"{"result":"partial"}"#),
        event(4, "mastery.assessed", r#"{"result":"ungraded"}"#),
        event(5, "mastery.assessed", "not json"),
        event(6, "mastery.assessed", r#"{"result":"correct"}"#),
    ];
    // Values 0, 0.5, 1 with weights 0.85, 0.95, 1.0: (0 + 0.475 + 1.0) / 2.8
    close(mastery_level(&log), 1.475 / 2.8);
}
