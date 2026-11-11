//! QoderDriver through a fake child process that speaks the schema-derived
//! stream and prints the recorded `--help`. No network, no quota.

use std::path::Path;
use std::time::Duration;

use serde_json::json;
use tutordocs_agent::qoder::QoderDriver;
use tutordocs_agent::{AgentDriver, AgentEvent, AuthKind, JobSpec};

mod common;
use common::{collect_bounded, install_agent, read_argv, FakeAgent};

const HELP: &str = include_str!("fixtures/qoder/help.txt");
const STREAM: &str = include_str!("fixtures/qoder/stream-from-schema.ndjson");
const ERROR: &str = include_str!("fixtures/qoder/error-from-schema.ndjson");

fn fake<'a>(help: &'a str, stream: &'a str) -> FakeAgent<'a> {
    FakeAgent {
        version: "1.1.85",
        help,
        help_on_stderr: false,
        stream,
    }
}

fn driver(dir: &Path, stream: &str, vars: &[(&str, &str)]) -> QoderDriver {
    QoderDriver::new(install_agent(dir, &fake(HELP, stream), vars))
        .expect("fake CLI probes cleanly")
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf-8 temp path")
}

fn answer_schema() -> serde_json::Value {
    json!({"type":"object","properties":{"answer":{"type":"integer"}},"required":["answer"]})
}

fn result_stream(text: &str) -> String {
    let init = STREAM.lines().next().unwrap();
    let result = json!({
        "type":"result","subtype":"success","is_error":false,"result":text,"session_id":"qoder-sess-1"
    });
    format!("{init}\n{result}\n")
}

#[tokio::test]
async fn construction_probes_version_and_capabilities_from_the_cli() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), STREAM, &[]);
    let caps = d.capabilities();
    assert_eq!(d.id(), "qoder");
    assert_eq!(d.version(), "1.1.85");
    assert!(caps.headless && caps.stream_json && caps.resume && caps.system_append);
    assert!(!caps.structured_output);
    assert_eq!(caps.auth, AuthKind::SubscriptionOAuth);
}

#[tokio::test]
async fn start_streams_session_text_result_done_and_spawns_the_pure_argv() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("argv.bin");
    let d = driver(tmp.path(), STREAM, &[("FAKE_ARGV_DUMP", path_str(&dump))]);
    let h = d.start(JobSpec::new("hello", tmp.path())).await.unwrap();
    assert!(h.pid.is_some());
    let events = collect_bounded(&mut d.events(&h)).await;
    assert_eq!(
        events,
        vec![
            AgentEvent::Session {
                id: "qoder-sess-1".into()
            },
            AgentEvent::TextDelta("{\"answer\":42}".into()),
            AgentEvent::MessageBoundary,
            AgentEvent::Result {
                value: json!({"answer": 42})
            },
            AgentEvent::Done,
        ]
    );
    assert_eq!(
        read_argv(&dump),
        [
            "-p",
            "--output-format",
            "stream-json",
            "--permission-mode",
            "dont_ask",
            "--",
            "hello"
        ]
    );
}

#[tokio::test]
async fn resume_passes_the_session_id() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("argv.bin");
    let d = driver(tmp.path(), STREAM, &[("FAKE_ARGV_DUMP", path_str(&dump))]);
    let h = d
        .resume("sess-xyz", JobSpec::new("go on", tmp.path()))
        .await
        .unwrap();
    let _ = collect_bounded(&mut d.events(&h)).await;
    let argv = read_argv(&dump);
    let pos = argv.iter().position(|a| a == "--resume").unwrap();
    assert_eq!(argv[pos + 1], "sess-xyz");
}

#[tokio::test]
async fn a_conforming_structured_reply_is_a_result() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), &result_stream("{\"answer\": 42}"), &[]);
    let mut job = JobSpec::new("answer", tmp.path());
    job.json_schema = Some(answer_schema());
    let h = d.start(job).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(events.contains(&AgentEvent::Result {
        value: json!({"answer": 42})
    }));
}

#[tokio::test]
async fn a_fenced_structured_reply_is_a_result() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(
        tmp.path(),
        &result_stream("```json\n{\"answer\": 42}\n```"),
        &[],
    );
    let mut job = JobSpec::new("answer", tmp.path());
    job.json_schema = Some(answer_schema());
    let h = d.start(job).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(events.contains(&AgentEvent::Result {
        value: json!({"answer": 42})
    }));
}

#[tokio::test]
async fn a_reply_that_breaks_the_schema_is_a_reported_failure_not_a_pass() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, reply) in [
        ("prose", "I think the answer is 42."),
        ("wrong type", "{\"answer\": \"forty-two\"}"),
        ("missing field", "{\"other\": 1}"),
    ] {
        let dir = tmp.path().join(name.replace(' ', "-"));
        std::fs::create_dir(&dir).unwrap();
        let d = driver(&dir, &result_stream(reply), &[]);
        let mut job = JobSpec::new("answer", tmp.path());
        job.json_schema = Some(answer_schema());
        let h = d.start(job).await.unwrap();
        let events = collect_bounded(&mut d.events(&h)).await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AgentEvent::Error(m) if m.contains("schema validation"))),
            "{name}: expected a schema failure, got {events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, AgentEvent::Result { .. })),
            "{name}: a bad reply must not surface as a Result"
        );
    }
}

#[tokio::test]
async fn the_schema_instruction_reaches_the_cli_in_the_prompt() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("argv.bin");
    let d = driver(tmp.path(), STREAM, &[("FAKE_ARGV_DUMP", path_str(&dump))]);
    let mut job = JobSpec::new("answer", tmp.path());
    job.json_schema = Some(json!({"type":"integer"}));
    let h = d.start(job).await.unwrap();
    let _ = collect_bounded(&mut d.events(&h)).await;
    let argv = read_argv(&dump);
    assert!(!argv.iter().any(|a| a == "--json-schema"));
    assert!(argv
        .last()
        .unwrap()
        .contains("JSON Schema: {\"type\":\"integer\"}"));
}

#[tokio::test]
async fn a_plain_text_warning_on_stdout_does_not_fail_a_successful_job() {
    let tmp = tempfile::tempdir().unwrap();
    let recorded = include_str!("fixtures/qoder/stream-with-skill-warning.ndjson");
    let d = driver(tmp.path(), recorded, &[]);
    let h = d.start(JobSpec::new("hi", tmp.path())).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(
        !events.iter().any(|e| matches!(e, AgentEvent::Error(_))),
        "events were: {events:?}"
    );
    assert!(events.contains(&AgentEvent::Result {
        value: json!({"answer": 42})
    }));
    assert_eq!(events.last(), Some(&AgentEvent::Done));
}

#[tokio::test]
async fn a_job_that_ends_without_a_result_event_fails_with_its_last_output() {
    let tmp = tempfile::tempdir().unwrap();
    let init = STREAM.lines().next().unwrap();
    let stream = format!("{init}\nlogin expired, run qodercli to sign in\n");
    let d = driver(tmp.path(), &stream, &[]);
    let h = d.start(JobSpec::new("hi", tmp.path())).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::Error(m)
            if m.contains("without a result") && m.contains("login expired"))),
        "events were: {events:?}"
    );
    assert_eq!(events.last(), Some(&AgentEvent::Done));
}

#[tokio::test]
async fn a_failed_run_surfaces_the_cli_error_and_done() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), ERROR, &[]);
    let h = d.start(JobSpec::new("hi", tmp.path())).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(events.contains(&AgentEvent::Error(
        "model request failed: quota exhausted".into()
    )));
    assert_eq!(events.last(), Some(&AgentEvent::Done));
}

#[tokio::test]
async fn a_nonzero_exit_reports_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(
        tmp.path(),
        "",
        &[("FAKE_MODE", "fail"), ("FAKE_STDERR", "login required")],
    );
    let h = d.start(JobSpec::new("hi", tmp.path())).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::Error(m) if m.contains("qoder exited") && m.contains("login required"))),
        "got {events:?}"
    );
}

#[tokio::test]
async fn a_cli_without_print_refuses_to_start_instead_of_guessing_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let d = QoderDriver::new(install_agent(
        tmp.path(),
        &fake("Usage: qodercli [options]\n  -v, --version\n", STREAM),
        &[],
    ))
    .unwrap();
    assert!(!d.capabilities().headless);
    let err = match d.start(JobSpec::new("hi", tmp.path())).await {
        Ok(_) => panic!("start must fail when --print is not advertised"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("--print"), "got: {err}");
}

#[tokio::test]
async fn resume_is_refused_when_help_lost_the_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let help: String = HELP
        .lines()
        .filter(|l| !l.contains("--resume"))
        .collect::<Vec<_>>()
        .join("\n");
    let d = QoderDriver::new(install_agent(tmp.path(), &fake(&help, STREAM), &[])).unwrap();
    let err = match d.resume("sess", JobSpec::new("hi", tmp.path())).await {
        Ok(_) => panic!("resume must fail without --resume"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("--resume"), "got: {err}");
}

#[tokio::test]
async fn a_hung_child_is_killed_at_the_job_timeout() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), STREAM, &[("FAKE_MODE", "hang")]);
    let mut job = JobSpec::new("hang", tmp.path());
    job.timeout = Some(Duration::from_millis(300));
    let h = d.start(job).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Error(m) if m.contains("timed out"))));
    assert_eq!(events.last(), Some(&AgentEvent::Done));
}
