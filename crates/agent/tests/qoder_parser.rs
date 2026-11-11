//! Qoder stream-json lines through the Qoder parser. The fixtures are built
//! from the message schemas embedded in the qodercli 1.1.85 binary, not from a
//! recorded run.

use serde_json::json;
use tutordocs_agent::qoder::QoderParser;
use tutordocs_agent::AgentEvent;

const STREAM: &str = include_str!("fixtures/qoder/stream-from-schema.ndjson");
const ERROR: &str = include_str!("fixtures/qoder/error-from-schema.ndjson");

fn feed(fixture: &str) -> Vec<AgentEvent> {
    let mut p = QoderParser::new();
    fixture.lines().flat_map(|l| p.parse_line(l)).collect()
}

fn result_line(text: &str) -> String {
    json!({"type":"result","subtype":"success","is_error":false,"result":text}).to_string()
}

#[test]
fn a_successful_run_yields_session_text_and_the_parsed_result() {
    assert_eq!(
        feed(STREAM),
        vec![
            AgentEvent::Session {
                id: "qoder-sess-1".into()
            },
            AgentEvent::TextDelta("{\"answer\":42}".into()),
            AgentEvent::MessageBoundary,
            AgentEvent::Result {
                value: json!({"answer": 42})
            },
        ]
    );
}

#[test]
fn a_tool_use_keeps_the_detail_the_claude_shaped_parser_reads() {
    let mut p = QoderParser::new();
    let line = json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"WebFetch","input":{"url":"https://example.com/a"}}]}}).to_string();
    assert_eq!(
        p.parse_line(&line),
        vec![AgentEvent::ToolUse {
            name: "WebFetch".into(),
            detail: Some("https://example.com/a".into())
        }]
    );
}

#[test]
fn a_failed_run_reports_the_errors_array_not_just_the_subtype() {
    assert_eq!(
        feed(ERROR),
        vec![
            AgentEvent::Session {
                id: "qoder-sess-2".into()
            },
            AgentEvent::Error("model request failed: quota exhausted".into()),
        ]
    );
}

#[test]
fn a_failed_run_with_no_detail_falls_back_to_the_subtype() {
    let mut p = QoderParser::new();
    let line = json!({"type":"result","subtype":"error_max_turns","is_error":true}).to_string();
    assert_eq!(
        p.parse_line(&line),
        vec![AgentEvent::Error("error_max_turns".into())]
    );
}

#[test]
fn a_result_wrapped_in_a_json_fence_is_unwrapped() {
    let mut p = QoderParser::new();
    let line = result_line("```json\n{\"answer\": 42}\n```");
    assert_eq!(
        p.parse_line(&line),
        vec![AgentEvent::Result {
            value: json!({"answer": 42})
        }]
    );
}

#[test]
fn prose_around_a_fence_is_not_guessed_at() {
    let mut p = QoderParser::new();
    let text = "Here you go:\n```json\n{\"answer\": 42}\n```";
    assert_eq!(
        p.parse_line(&result_line(text)),
        vec![AgentEvent::Result { value: json!(text) }]
    );
}

#[test]
fn a_fence_holding_invalid_json_stays_text_for_validation_to_reject() {
    let mut p = QoderParser::new();
    let text = "```json\n{answer: 42\n```";
    assert_eq!(
        p.parse_line(&result_line(text)),
        vec![AgentEvent::Result { value: json!(text) }]
    );
}

#[test]
fn a_native_structured_output_wins_over_the_result_text() {
    let mut p = QoderParser::new();
    let line = json!({
        "type":"result","subtype":"success","is_error":false,
        "result":"done","structured_output":{"answer":7}
    })
    .to_string();
    assert_eq!(
        p.parse_line(&line),
        vec![AgentEvent::Result {
            value: json!({"answer": 7})
        }]
    );
}

#[test]
fn unknown_and_malformed_lines_do_not_break_the_stream() {
    let mut p = QoderParser::new();
    assert_eq!(p.parse_line(r#"{"type":"future_thing"}"#), vec![]);
    assert_eq!(p.parse_line("not json"), vec![]);
}

#[test]
fn a_plain_text_warning_on_stdout_is_not_an_error() {
    let recorded = include_str!("fixtures/qoder/stream-with-skill-warning.ndjson");
    let warning = recorded.lines().last().unwrap();
    assert!(warning.starts_with("Skill command '/review' was renamed"));
    assert_eq!(QoderParser::new().parse_line(warning), vec![]);
}
