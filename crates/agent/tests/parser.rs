//! Parser tests fed recorded-shape NDJSON fixture lines. No network, no quota.

use serde_json::json;
use tutordocs_agent::parse::NdjsonParser;
use tutordocs_agent::{fold_events, AgentEvent, Folded};

const FIXTURE: &str = include_str!("fixtures/stream.ndjson");

fn line(n: usize) -> &'static str {
    FIXTURE.lines().nth(n).expect("fixture line exists")
}

#[test]
fn init_yields_session_exactly_once() {
    let mut p = NdjsonParser::new();
    assert_eq!(
        p.parse_line(line(0)),
        vec![AgentEvent::Session {
            id: "sess-abc123".to_string()
        }]
    );
    // Later lines carry the same session id but must not re-emit it.
    assert!(p
        .parse_line(line(3))
        .iter()
        .all(|e| !matches!(e, AgentEvent::Session { .. })));
}

#[test]
fn deltas_stream_and_assistant_text_is_not_doubled() {
    let mut p = NdjsonParser::new();
    p.parse_line(line(0));
    assert_eq!(
        p.parse_line(line(1)),
        vec![AgentEvent::TextDelta("Hel".into())]
    );
    assert_eq!(
        p.parse_line(line(2)),
        vec![AgentEvent::TextDelta("lo".into())]
    );
    // Full assistant text after deltas would double the output: suppressed.
    assert_eq!(p.parse_line(line(3)), vec![AgentEvent::MessageBoundary]);
    assert_eq!(
        p.parse_line(line(4)),
        vec![AgentEvent::ToolUse {
            name: "Read".into(),
            detail: None
        }]
    );
}

#[test]
fn assistant_text_emits_when_no_partials_were_seen() {
    let mut p = NdjsonParser::new();
    p.parse_line(line(0));
    assert_eq!(
        p.parse_line(line(3)),
        vec![
            AgentEvent::TextDelta("Hello".into()),
            AgentEvent::MessageBoundary
        ]
    );
}

#[test]
fn tool_result_and_unknown_lines_are_ignored() {
    let mut p = NdjsonParser::new();
    assert_eq!(p.parse_line(line(5)), vec![]);
    assert_eq!(
        p.parse_line(r#"{"type":"future_thing","payload":1}"#),
        vec![]
    );
}

#[test]
fn success_result_wraps_plain_text() {
    let mut p = NdjsonParser::new();
    let events = p.parse_line(line(6));
    assert_eq!(
        events,
        vec![AgentEvent::Result {
            value: json!("Hello")
        }]
    );
}

#[test]
fn success_result_parses_json_payload() {
    let mut p = NdjsonParser::new();
    let l = r#"{"type":"result","subtype":"success","is_error":false,"result":"{\"answer\":42}","session_id":"s"}"#;
    assert_eq!(
        p.parse_line(l),
        vec![AgentEvent::Result {
            value: json!({"answer": 42})
        }]
    );
}

#[test]
fn error_result_yields_error_event() {
    let mut p = NdjsonParser::new();
    let l = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"boom","session_id":"s"}"#;
    assert_eq!(p.parse_line(l), vec![AgentEvent::Error("boom".into())]);
}

#[test]
fn a_non_json_line_yields_no_event_and_parser_recovers() {
    let mut p = NdjsonParser::new();
    assert_eq!(p.parse_line("this is not json"), vec![]);
    // The parser still works on the next valid line.
    assert_eq!(
        p.parse_line(line(0)),
        vec![AgentEvent::Session {
            id: "sess-abc123".into()
        }]
    );
}

#[test]
fn blank_lines_are_ignored() {
    let mut p = NdjsonParser::new();
    assert_eq!(p.parse_line(""), vec![]);
    assert_eq!(p.parse_line("   "), vec![]);
}

#[test]
fn full_fixture_sequence() {
    let mut p = NdjsonParser::new();
    let events: Vec<AgentEvent> = FIXTURE.lines().flat_map(|l| p.parse_line(l)).collect();
    assert_eq!(
        events,
        vec![
            AgentEvent::Session {
                id: "sess-abc123".into()
            },
            AgentEvent::TextDelta("Hel".into()),
            AgentEvent::TextDelta("lo".into()),
            AgentEvent::MessageBoundary,
            AgentEvent::ToolUse {
                name: "Read".into(),
                detail: None
            },
            AgentEvent::Result {
                value: json!("Hello")
            },
        ]
    );
}

#[test]
fn structured_output_field_wins_over_the_prose_result_text() {
    // With --json-schema the CLI reports the validated object in
    // `structured_output`; `result` can be prose or empty.
    let mut p = NdjsonParser::new();
    let raw = r#"{"type":"result","subtype":"success","is_error":false,"result":"Done.","structured_output":{"answer":42},"session_id":"s"}"#;
    assert_eq!(
        p.parse_line(raw),
        vec![AgentEvent::Result {
            value: json!({"answer": 42})
        }]
    );
}

#[test]
fn error_result_ignores_structured_output() {
    let mut p = NdjsonParser::new();
    let raw = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"result":"","structured_output":{"answer":1},"session_id":"s"}"#;
    assert_eq!(
        p.parse_line(raw),
        vec![AgentEvent::Error("error_max_turns".to_string())]
    );
}

const TOOLS: &str = include_str!("fixtures/claude/tools.ndjson");

#[test]
fn each_named_tool_keeps_its_short_detail() {
    let mut p = NdjsonParser::new();
    let details: Vec<_> = TOOLS
        .lines()
        .map(|line| {
            let events = p.parse_line(line);
            let AgentEvent::ToolUse { name, detail } = &events[0] else {
                panic!("expected a tool, got {events:?}");
            };
            (name.clone(), detail.clone())
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
    let mut p = NdjsonParser::new();
    let events: Vec<_> = FIXTURE
        .lines()
        .flat_map(|line| p.parse_line(line))
        .collect();
    assert_eq!(
        fold_events(&events),
        vec![
            Folded::Text("Hello".into()),
            Folded::Tool {
                name: "Read".into(),
                detail: None
            },
        ]
    );
}
