//! agy stream-json lines through the agy parser. The fixtures are verbatim
//! examples from the official headless page, not recorded runs.

use serde_json::json;
use tutordocs_agent::agy::AgyParser;
use tutordocs_agent::{fold_events, AgentEvent, Folded};

const STREAM: &str = include_str!("fixtures/agy/stream-from-docs.ndjson");
const TOOL: &str = include_str!("fixtures/agy/tool-step-from-docs.ndjson");
const STRUCTURED: &str = include_str!("fixtures/agy/structured-from-docs.ndjson");
const ERROR: &str = include_str!("fixtures/agy/error-from-docs.ndjson");

const REBASE: &str = "Git rebase destructively rewrites a branch's commit history by systematically detaching its unique commits and sequentially reapplying them onto a new base commit.\n";

fn feed(fixture: &str) -> Vec<AgentEvent> {
    let mut p = AgyParser::new();
    fixture.lines().flat_map(|l| p.parse_line(l)).collect()
}

#[test]
fn a_successful_run_yields_session_text_and_result() {
    assert_eq!(
        feed(STREAM),
        vec![
            AgentEvent::Session {
                id: "c3b66b04-872b-4fbe-a3a4-058a026ef20a".into()
            },
            AgentEvent::TextDelta(REBASE.into()),
            AgentEvent::MessageBoundary,
            AgentEvent::Result {
                value: json!(REBASE)
            },
        ]
    );
}

#[test]
fn a_tool_step_yields_the_tool_name() {
    assert_eq!(
        feed(TOOL),
        vec![AgentEvent::ToolUse {
            name: "run_command".into(),
            detail: None
        }]
    );
}

#[test]
fn a_tool_step_seen_active_then_done_is_reported_once() {
    let mut p = AgyParser::new();
    let line = |state: &str| {
        json!({"event":"step_update","step_update":{
            "step_index":4,"state":state,"step_type":"tool","tool_name":"run_command"
        }})
        .to_string()
    };
    assert_eq!(p.parse_line(&line("ACTIVE")).len(), 1);
    assert_eq!(p.parse_line(&line("DONE")), vec![]);
}

#[test]
fn a_structured_output_wins_over_the_response_text() {
    assert_eq!(
        feed(STRUCTURED),
        vec![
            AgentEvent::Session {
                id: "4e502687-290c-4030-b908-5ed6c68fa5dc".into()
            },
            AgentEvent::Result {
                value: json!({"major": 2, "minor": 14, "patch": 3})
            },
        ]
    );
}

#[test]
fn a_response_that_is_json_text_is_parsed_when_no_structured_output_came() {
    let mut p = AgyParser::new();
    let line = json!({"event":"result","result":{
        "status":"SUCCESS","response":"```json\n{\"answer\": 42}\n```\n"
    }})
    .to_string();
    assert_eq!(
        p.parse_line(&line),
        vec![AgentEvent::Result {
            value: json!({"answer": 42})
        }]
    );
}

#[test]
fn an_error_status_reports_the_error_text_and_no_result() {
    let events = feed(ERROR);
    assert_eq!(events.len(), 1, "an empty conversation_id emits no Session");
    assert!(
        matches!(&events[0], AgentEvent::Error(m) if m.starts_with("invalid model selection")),
        "got {events:?}"
    );
}

#[test]
fn a_non_success_status_without_an_error_field_still_reports_a_failure() {
    let mut p = AgyParser::new();
    for status in ["CANCELED", "INTERRUPTED", "INVALID", "WAITING", "RUNNING"] {
        let line = json!({"event":"result","result":{"status":status,"response":""}}).to_string();
        assert_eq!(
            p.parse_line(&line),
            vec![AgentEvent::Error(format!(
                "agy run ended with status {status}"
            ))]
        );
    }
}

#[test]
fn a_result_with_no_status_is_a_failure_not_a_silent_pass() {
    let mut p = AgyParser::new();
    let line = json!({"event":"result","result":{"response":"hi"}}).to_string();
    assert!(matches!(
        p.parse_line(&line).as_slice(),
        [AgentEvent::Error(m)] if m.contains("no status")
    ));
}

#[test]
fn unknown_events_are_ignored_and_malformed_lines_reported() {
    let mut p = AgyParser::new();
    assert_eq!(p.parse_line(r#"{"event":"future_thing","x":1}"#), vec![]);
    assert_eq!(p.parse_line(""), vec![]);
    assert_eq!(p.parse_line("not json"), vec![]);
}

#[test]
fn session_is_emitted_once() {
    let mut p = AgyParser::new();
    let init = r#"{"event":"init","conversation_id":"conv-1","init":{}}"#;
    assert_eq!(p.parse_line(init).len(), 1);
    assert_eq!(p.parse_line(init), vec![]);
}

const NAMED_TOOLS: &str = include_str!("fixtures/agy/tools.ndjson");
const TEXT_DELTAS: &str = include_str!("fixtures/agy/text-deltas.ndjson");

#[test]
fn each_named_tool_keeps_its_short_detail() {
    let details: Vec<_> = feed(NAMED_TOOLS)
        .into_iter()
        .map(|event| {
            let AgentEvent::ToolUse { name, detail } = event else {
                panic!("expected a tool, got {event:?}");
            };
            (name, detail)
        })
        .collect();
    assert_eq!(
        details,
        vec![
            ("Read".into(), Some("sources/notes.md".into())),
            ("Write".into(), Some("chapter/index.md".into())),
            ("Edit".into(), Some("chapter/index.md".into())),
            ("Grep".into(), Some("fn main".into())),
            ("Glob".into(), Some("sources/**/*.md".into())),
            ("WebFetch".into(), Some("https://example.com/notes".into())),
        ]
    );
}

#[test]
fn text_deltas_of_one_assistant_message_fold_into_one_line() {
    assert_eq!(
        fold_events(&feed(TEXT_DELTAS)),
        vec![
            Folded::Text("Hello".into()),
            Folded::Tool {
                name: "Read".into(),
                detail: Some("sources/notes.md".into())
            },
        ]
    );
}
