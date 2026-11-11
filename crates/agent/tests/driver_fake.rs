//! The AgentDriver trait exercised through a fake child process: a shell
//! script that speaks the recorded NDJSON shapes. No network, no quota.
//! Each test installs its own fake with its steering baked in, so nothing
//! here touches process env and the tests run in parallel.

use std::time::Duration;

use serde_json::json;
use tutordocs_agent::claude::ClaudeDriver;
use tutordocs_agent::{AgentDriver, AgentEvent, AuthKind, JobSpec};

mod common;
use common::{collect_bounded, install_fake};

fn driver(dir: &std::path::Path, vars: &[(&str, &str)]) -> ClaudeDriver {
    ClaudeDriver::new(install_fake(dir, vars)).expect("fake CLI probes cleanly")
}

fn path_str(path: &std::path::Path) -> &str {
    path.to_str().expect("utf-8 temp path")
}

#[tokio::test]
async fn capabilities_are_probed_not_hardcoded() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), &[]);
    let caps = d.capabilities();
    assert_eq!(d.id(), "claude");
    assert!(caps.headless && caps.stream_json && caps.structured_output);
    assert!(caps.resume && caps.system_append && caps.tool_allowlist);
    assert_eq!(caps.auth, AuthKind::SubscriptionOAuth);
}

#[tokio::test]
async fn capabilities_degrade_when_help_lacks_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), &[("FAKE_CLAUDE_HELP_MODE", "minimal")]);
    let caps = d.capabilities();
    assert!(caps.headless);
    assert!(!caps.stream_json, "no stream-json in minimal help");
    assert!(!caps.structured_output, "no --json-schema in minimal help");
    assert!(!caps.resume);
}

#[tokio::test]
async fn a_degraded_cli_refuses_to_start_instead_of_guessing_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), &[("FAKE_CLAUDE_HELP_MODE", "minimal")]);
    let err = match d.start(JobSpec::new("hi", tmp.path())).await {
        Ok(_) => panic!("start must fail when stream-json is not advertised"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("stream-json"), "got: {err}");
}

#[tokio::test]
async fn an_allowlist_the_cli_cannot_enforce_is_refused_before_spawning() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("argv");
    let d = driver(
        tmp.path(),
        &[
            ("FAKE_CLAUDE_HELP_MODE", "no_tools"),
            ("FAKE_CLAUDE_ARGV_DUMP", path_str(&dump)),
        ],
    );
    assert!(!d.capabilities().tool_allowlist);
    let mut job = JobSpec::new("hi", tmp.path());
    job.allowed_tools = Some(vec!["WebFetch".into()]);
    let err = match d.start(job).await {
        Ok(_) => panic!("start must refuse an allowlist it cannot enforce"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("tool allowlist"), "got: {err}");
    assert!(!dump.exists(), "the CLI must not be spawned");
}

#[tokio::test]
async fn start_streams_session_text_result_done() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), &[]);
    let h = d
        .start(JobSpec::new("say hello", tmp.path()))
        .await
        .unwrap();
    assert!(h.pid.is_some());
    let events = collect_bounded(&mut d.events(&h)).await;
    assert_eq!(
        events,
        vec![
            AgentEvent::Session {
                id: "sess-fake".into()
            },
            AgentEvent::TextDelta("Hello".into()),
            AgentEvent::MessageBoundary,
            AgentEvent::Result {
                value: json!("Hello")
            },
            AgentEvent::Done,
        ]
    );
}

#[tokio::test]
async fn argv_carries_the_contract_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("argv.txt");
    let d = driver(tmp.path(), &[("FAKE_CLAUDE_ARGV_DUMP", path_str(&dump))]);
    let mut job = JobSpec::new("write a chapter", tmp.path());
    job.json_schema = Some(
        json!({"type":"object","properties":{"answer":{"type":"integer"}},"required":["answer"]}),
    );
    job.system_append = Some("You are a textbook author.".into());
    job.allowed_tools = Some(vec!["Read".into(), "Grep".into(), "Glob".into()]);
    let h = d.start(job).await.unwrap();
    let _ = collect_bounded(&mut d.events(&h)).await;

    let argv = std::fs::read_to_string(&dump).unwrap();
    let lines: Vec<&str> = argv.lines().collect();
    for expected in [
        "-p",
        "write a chapter",
        "--output-format",
        "stream-json",
        "--verbose",
        "--json-schema",
        "--append-system-prompt",
        "You are a textbook author.",
        "--permission-mode",
        "plan",
        "--dangerously-skip-permissions",
        "--strict-mcp-config",
        "--settings",
        "{\"disableAllHooks\":true}",
        "--include-hook-events",
    ] {
        assert!(lines.contains(&expected), "argv missing {expected}: {argv}");
    }
}

#[tokio::test]
async fn resume_passes_the_session_id() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("argv.txt");
    let d = driver(tmp.path(), &[("FAKE_CLAUDE_ARGV_DUMP", path_str(&dump))]);
    let h = d
        .resume("sess-xyz", JobSpec::new("continue", tmp.path()))
        .await
        .unwrap();
    let _ = collect_bounded(&mut d.events(&h)).await;

    let argv = std::fs::read_to_string(&dump).unwrap();
    let lines: Vec<&str> = argv.lines().collect();
    let pos = lines
        .iter()
        .position(|l| *l == "--resume")
        .expect("--resume present");
    assert_eq!(lines[pos + 1], "sess-xyz");
}

#[tokio::test]
async fn structured_result_is_validated_against_the_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let schema =
        json!({"type":"object","properties":{"answer":{"type":"integer"}},"required":["answer"]});

    let good_dir = tmp.path().join("good");
    std::fs::create_dir(&good_dir).unwrap();
    let d = driver(&good_dir, &[("FAKE_CLAUDE_MODE", "goodjson")]);
    let mut job = JobSpec::new("answer", tmp.path());
    job.json_schema = Some(schema.clone());
    let h = d.start(job).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(events.contains(&AgentEvent::Result {
        value: json!({"answer":42})
    }));

    let bad_dir = tmp.path().join("bad");
    std::fs::create_dir(&bad_dir).unwrap();
    let d = driver(&bad_dir, &[("FAKE_CLAUDE_MODE", "badjson")]);
    let mut job = JobSpec::new("answer", tmp.path());
    job.json_schema = Some(schema);
    let h = d.start(job).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::Error(m) if m.contains("schema"))),
        "schema violation must surface as Error, got {events:?}"
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, AgentEvent::Result { .. })));
}

fn pid_is_gone(pid: u32) -> bool {
    !std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[tokio::test]
async fn cancel_kills_the_child_and_leaves_no_orphan() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), &[("FAKE_CLAUDE_MODE", "hang")]);
    let h = d.start(JobSpec::new("hang", tmp.path())).await.unwrap();
    let pid = h.pid.expect("child pid");
    d.cancel(&h).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(
        events.contains(&AgentEvent::Done),
        "pump must finish after cancel"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AgentEvent::Error(m) if m.contains("timed out"))),
        "cancel ends the job without a timeout error, got {events:?}"
    );
    assert!(pid_is_gone(pid), "child {pid} survived cancel");
}

/// The old chapter bound was 900s and the old default was 120s. Pausing the
/// clock and stepping past both must leave a limitless job alive, and releasing
/// the child must still let it succeed.
#[tokio::test(start_paused = true)]
async fn a_job_with_no_time_limit_finishes_after_the_old_bounds() {
    let tmp = tempfile::tempdir().unwrap();
    let release = tmp.path().join("release");
    let d = driver(
        tmp.path(),
        &[
            ("FAKE_CLAUDE_MODE", "release"),
            ("FAKE_CLAUDE_RELEASE", path_str(&release)),
        ],
    );
    let job = JobSpec::new("slow", tmp.path());
    assert!(job.timeout.is_none());
    let h = d.start(job).await.unwrap();
    let pid = h.pid.expect("child pid");
    let mut stream = d.events(&h);
    let first = stream.recv().await;
    assert!(
        matches!(first, Some(AgentEvent::Session { .. })),
        "the pump must be reading before the clock jumps, got {first:?}"
    );

    tokio::time::advance(Duration::from_secs(901)).await;
    assert!(
        !pid_is_gone(pid),
        "child {pid} died when the clock passed the old 900s bound"
    );

    std::fs::write(&release, "go").unwrap();
    // The jump above is paused time. Resume so the child's real sleep can
    // finish and the 5s collect bound is wall-clock time again.
    tokio::time::resume();
    let mut events = vec![first.unwrap()];
    events.extend(collect_bounded(&mut stream).await);
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::Result { value } if value == "Finished after the wait."
        )),
        "the job must succeed after the wait, got {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AgentEvent::Error(m) if m.contains("timed out"))),
        "a limitless job must not report a timeout, got {events:?}"
    );
    assert!(events.contains(&AgentEvent::Done));
}

#[tokio::test]
async fn timeout_kills_the_child_and_reports() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), &[("FAKE_CLAUDE_MODE", "hang")]);
    let mut job = JobSpec::new("hang", tmp.path());
    job.timeout = Some(Duration::from_millis(300));
    let h = d.start(job).await.unwrap();
    let pid = h.pid.expect("child pid");
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::Error(m) if m.contains("timed out"))),
        "expected timeout error, got {events:?}"
    );
    assert!(events.contains(&AgentEvent::Done));
    assert!(pid_is_gone(pid), "child {pid} survived the timeout");
}

#[tokio::test]
async fn ndjson_tap_records_the_raw_stdout_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let tap = tmp.path().join("raw.ndjson");
    let d = driver(tmp.path(), &[]).with_ndjson_tap(&tap);
    let h = d.start(JobSpec::new("hi", tmp.path())).await.unwrap();
    let _ = collect_bounded(&mut d.events(&h)).await;

    let raw = std::fs::read_to_string(&tap).unwrap();
    let kinds: Vec<String> = raw
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).expect("tap holds raw JSON lines");
            v["type"].as_str().unwrap().to_string()
        })
        .collect();
    assert_eq!(kinds, ["system", "assistant", "result"]);
}

#[tokio::test]
async fn an_unwritable_tap_fails_start_without_spawning() {
    let tmp = tempfile::tempdir().unwrap();
    let tap = tmp.path().join("no-such-dir").join("raw.ndjson");
    let d = driver(tmp.path(), &[]).with_ndjson_tap(&tap);
    let err = match d.start(JobSpec::new("hi", tmp.path())).await {
        Ok(_) => panic!("start must fail when the tap cannot be created"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("NDJSON tap"), "got: {err}");
}
