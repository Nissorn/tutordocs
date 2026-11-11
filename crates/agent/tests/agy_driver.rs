//! AgyDriver through a fake child process that prints the recorded `--help`
//! on stderr, as the real agy does, and speaks the documented stream.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use tutordocs_agent::agy::AgyDriver;
use tutordocs_agent::{AgentDriver, AgentEvent, AuthKind, JobSpec};

mod common;
use common::{collect_bounded, install_agent, passthrough_sandbox, read_argv, FakeAgent};

const HELP: &str = include_str!("fixtures/agy/help.stderr.txt");
const STREAM: &str = include_str!("fixtures/agy/stream-from-docs.ndjson");
const STRUCTURED: &str = include_str!("fixtures/agy/structured-from-docs.ndjson");
const ERROR: &str = include_str!("fixtures/agy/error-from-docs.ndjson");

const REBASE: &str = "Git rebase destructively rewrites a branch's commit history by systematically detaching its unique commits and sequentially reapplying them onto a new base commit.\n";

fn fake<'a>(help: &'a str, stream: &'a str) -> FakeAgent<'a> {
    FakeAgent {
        version: "1.5.1",
        help,
        help_on_stderr: true,
        stream,
    }
}

fn driver(dir: &Path, stream: &str, vars: &[(&str, &str)]) -> AgyDriver {
    let bin = install_agent(dir, &fake(HELP, stream), vars);
    let sandbox = passthrough_sandbox(dir, None, None);
    AgyDriver::new(bin)
        .expect("fake CLI probes cleanly")
        .with_sandbox_exec(sandbox)
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf-8 temp path")
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
async fn help_printed_on_stderr_still_probes_to_real_capabilities() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), STREAM, &[]);
    let caps = d.capabilities();
    assert_eq!(d.id(), "agy");
    assert_eq!(d.version(), "1.5.1");
    assert!(
        caps.headless && caps.stream_json && caps.structured_output && caps.resume,
        "a probe that read only stdout would see none of these: {caps:?}"
    );
    assert!(!caps.system_append);
    assert_eq!(caps.auth, AuthKind::SubscriptionOAuth);
    assert!(
        caps.read_only && caps.tool_allowlist,
        "the sandbox satisfies both, so a course job is not refused"
    );
}

#[tokio::test]
async fn start_streams_session_text_result_done_and_spawns_the_pure_argv() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("argv.bin");
    let d = driver(tmp.path(), STREAM, &[("FAKE_ARGV_DUMP", path_str(&dump))]);
    let h = d.start(writable("hello", tmp.path())).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert_eq!(
        events,
        vec![
            AgentEvent::Session {
                id: "c3b66b04-872b-4fbe-a3a4-058a026ef20a".into()
            },
            AgentEvent::TextDelta(REBASE.into()),
            AgentEvent::MessageBoundary,
            AgentEvent::Result {
                value: json!(REBASE)
            },
            AgentEvent::Done,
        ]
    );
    assert_eq!(
        read_argv(&dump),
        [
            "-p",
            "hello",
            "--output-format",
            "stream-json",
            "--dangerously-skip-permissions",
        ]
    );
}

#[tokio::test]
async fn resume_passes_the_conversation_id() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("argv.bin");
    let d = driver(tmp.path(), STREAM, &[("FAKE_ARGV_DUMP", path_str(&dump))]);
    let h = d
        .resume("conv-xyz", writable("go on", tmp.path()))
        .await
        .unwrap();
    let _ = collect_bounded(&mut d.events(&h)).await;
    let argv = read_argv(&dump);
    let pos = argv.iter().position(|a| a == "--conversation").unwrap();
    assert_eq!(argv[pos + 1], "conv-xyz");
}

#[tokio::test]
async fn a_native_structured_output_is_validated_against_the_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let good =
        json!({"type":"object","properties":{"major":{"type":"integer"}},"required":["major"]});
    let d = driver(tmp.path(), STRUCTURED, &[]);
    let mut job = writable("parse", tmp.path());
    job.json_schema = Some(good);
    let h = d.start(job).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(events.contains(&AgentEvent::Result {
        value: json!({"major": 2, "minor": 14, "patch": 3})
    }));

    let bad_dir = tmp.path().join("bad");
    std::fs::create_dir(&bad_dir).unwrap();
    let d = driver(&bad_dir, STRUCTURED, &[]);
    let mut job = writable("parse", tmp.path());
    job.json_schema = Some(json!({"type":"object","required":["not_there"]}));
    let h = d.start(job).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Error(m) if m.contains("schema validation"))));
    assert!(!events
        .iter()
        .any(|e| matches!(e, AgentEvent::Result { .. })));
}

#[tokio::test]
async fn an_error_status_is_a_reported_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), ERROR, &[]);
    let h = d.start(writable("hi", tmp.path())).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Error(m) if m.starts_with("invalid model selection"))));
    assert_eq!(events.last(), Some(&AgentEvent::Done));
}

#[tokio::test]
async fn a_nonzero_exit_reports_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(
        tmp.path(),
        "",
        &[
            ("FAKE_MODE", "fail"),
            ("FAKE_STDERR", "cannot reach the model endpoint"),
        ],
    );
    let h = d.start(writable("hi", tmp.path())).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::Error(m) if m.contains("agy exited") && m.contains("cannot reach the model endpoint"))),
        "got {events:?}"
    );
}

#[tokio::test]
async fn an_auth_failure_names_agy_and_the_sign_in_remedy() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(
        tmp.path(),
        "",
        &[
            ("FAKE_MODE", "fail"),
            (
                "FAKE_STDERR",
                "Authentication required. Please visit the URL to log in",
            ),
        ],
    );
    let h = d.start(writable("hi", tmp.path())).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    let error = events
        .iter()
        .find_map(|e| match e {
            AgentEvent::Error(m) => Some(m.clone()),
            _ => None,
        })
        .expect("a failed login is an error event");
    assert!(
        error.contains("run `agy` once in Terminal and sign in with the Google account"),
        "the remedy must name the command and the account: {error}"
    );
    assert!(
        !error.contains("Please visit the URL"),
        "the raw login prompt must not reach the job log: {error}"
    );
}

#[tokio::test]
async fn a_hung_agy_after_init_is_killed_at_the_job_timeout() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), STREAM, &[("FAKE_MODE", "hang")]);
    let mut job = writable("hang", tmp.path());
    // Two shells start before the init line: sandbox-exec, then the fake.
    // 300ms lost that line when the suite ran in parallel. 2s is still far
    // under the fake's 30s sleep, so only the timeout can end the job.
    job.timeout = Some(Duration::from_secs(2));
    let started = Instant::now();
    let h = d.start(job).await.unwrap();
    let pid = h.pid.expect("child pid");
    let events = collect_bounded(&mut d.events(&h)).await;

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the fake sleeps 30s, so only the timeout kill can end the job this fast"
    );
    assert!(
        matches!(events.first(), Some(AgentEvent::Session { .. })),
        "events seen before the hang are kept: {events:?}"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Error(m) if m.contains("timed out"))));
    assert_eq!(events.last(), Some(&AgentEvent::Done));
    assert!(pid_is_gone(pid), "child {pid} survived the timeout");
}

#[tokio::test]
async fn a_silent_hang_with_no_output_at_all_is_still_killed() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), STREAM, &[("FAKE_MODE", "hang_silent")]);
    let mut job = writable("hang", tmp.path());
    job.timeout = Some(Duration::from_millis(300));
    let h = d.start(job).await.unwrap();
    let pid = h.pid.expect("child pid");
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Error(m) if m.contains("timed out"))));
    assert!(pid_is_gone(pid), "child {pid} survived the timeout");
}

#[tokio::test]
async fn cancel_kills_the_child_and_leaves_no_orphan() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), STREAM, &[("FAKE_MODE", "hang_silent")]);
    let h = d.start(writable("hang", tmp.path())).await.unwrap();
    let pid = h.pid.expect("child pid");
    d.cancel(&h).await.unwrap();
    let events = collect_bounded(&mut d.events(&h)).await;
    assert!(events.contains(&AgentEvent::Done));
    assert!(pid_is_gone(pid), "child {pid} survived cancel");
}

/// A job the sandbox is allowed to run. `read_only` stays at the caller's
/// value; the driver no longer refuses it.
fn writable(prompt: &str, workdir: &std::path::Path) -> JobSpec {
    let mut job = JobSpec::new(prompt, workdir);
    job.read_only = false;
    job
}

#[tokio::test]
async fn a_read_only_job_runs_because_the_sandbox_confines_it() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), STREAM, &[]);
    let handle = d.start(JobSpec::new("hi", tmp.path())).await.unwrap();
    let events = collect_bounded(&mut d.events(&handle)).await;
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Result { .. })));
}

#[tokio::test]
async fn a_job_with_a_tool_allowlist_runs_because_the_sandbox_confines_it() {
    let tmp = tempfile::tempdir().unwrap();
    let d = driver(tmp.path(), STREAM, &[]);
    let mut job = writable("hi", tmp.path());
    job.allowed_tools = Some(vec!["WebFetch".into()]);
    let handle = d.start(job).await.unwrap();
    let events = collect_bounded(&mut d.events(&handle)).await;
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Result { .. })));
}

#[tokio::test]
async fn a_cli_without_print_refuses_to_start_instead_of_guessing_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let d = AgyDriver::new(install_agent(
        tmp.path(),
        &fake("Usage of agy:\n  --model  Model\n", STREAM),
        &[],
    ))
    .unwrap();
    assert!(!d.capabilities().headless);
    let err = match d.start(writable("hi", tmp.path())).await {
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
        .filter(|l| !l.contains("--conversation"))
        .collect::<Vec<_>>()
        .join("\n");
    let d = AgyDriver::new(install_agent(tmp.path(), &fake(&help, STREAM), &[])).unwrap();
    let err = match d.resume("conv", writable("hi", tmp.path())).await {
        Ok(_) => panic!("resume must fail without --conversation"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("--conversation"), "got: {err}");
}
