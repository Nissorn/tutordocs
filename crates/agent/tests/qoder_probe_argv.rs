//! Qoder seams that need no subprocess: the capability probe read from the
//! recorded `--help`, and argv as a pure function of the job.

use serde_json::json;
use tutordocs_agent::qoder::{build_args, QoderDriver};
use tutordocs_agent::{AuthKind, Capabilities, JobSpec};

const HELP: &str = include_str!("fixtures/qoder/help.txt");

fn recorded_caps() -> Capabilities {
    QoderDriver::capabilities_from_help(HELP)
}

fn job(prompt: &str) -> JobSpec {
    JobSpec::new(prompt, "/work")
}

#[test]
fn recorded_help_probes_to_the_documented_capabilities() {
    let caps = recorded_caps();
    assert!(caps.headless, "-p/--print is in the help");
    assert!(caps.stream_json, "--output-format is in the help");
    assert!(caps.resume && caps.tool_allowlist);
    assert!(caps.system_append, "--append-system-prompt is in the help");
    assert!(
        !caps.structured_output,
        "the 1.1.85 help lists no --json-schema"
    );
    assert_eq!(caps.auth, AuthKind::SubscriptionOAuth);
}

#[test]
fn recorded_help_advertises_a_tool_allowlist_and_a_read_only_mode() {
    let caps = recorded_caps();
    assert!(
        caps.tool_allowlist,
        "--tools and --allowed-tools are in the help"
    );
    assert!(caps.read_only, "dont_ask is a --permission-mode choice");
}

#[test]
fn an_allowlist_limits_the_tool_set_and_pre_approves_only_non_file_tools() {
    let mut j = job("fetch");
    j.read_only = false;
    j.allowed_tools = Some(vec!["WebFetch".into(), "Write".into()]);
    let args = build_args(&j, None, &recorded_caps());
    let after = |flag: &str| {
        let pos = args.iter().position(|a| a == flag).unwrap();
        args[pos + 1].clone()
    };
    assert_eq!(after("--tools"), "WebFetch,Write");
    assert_eq!(after("--allowed-tools"), "WebFetch");
    assert_eq!(after("--permission-mode"), "accept_edits");
    let separator = args.iter().position(|a| a == "--").unwrap();
    let tools = args.iter().position(|a| a == "--tools").unwrap();
    assert!(
        tools < separator,
        "the variadic --tools ends before the prompt"
    );
}

#[test]
fn a_job_without_an_allowlist_passes_no_tool_flags() {
    let args = build_args(&job("hi"), None, &recorded_caps());
    assert!(!args
        .iter()
        .any(|a| a == "--tools" || a == "--allowed-tools"));
}

#[test]
fn a_flag_dropped_by_an_update_degrades_only_its_capability() {
    let without = |flag: &str| -> Capabilities {
        let kept: Vec<&str> = HELP.lines().filter(|l| !l.contains(flag)).collect();
        QoderDriver::capabilities_from_help(&kept.join("\n"))
    };
    let caps = without("--resume");
    assert!(!caps.resume);
    assert!(caps.headless && caps.stream_json && caps.system_append);

    let caps = without("--append-system-prompt");
    assert!(!caps.system_append);
    assert!(caps.headless && caps.resume);

    let caps = without("--output-format");
    assert!(!caps.stream_json);

    let caps = without("--tools <tools...>");
    assert!(!caps.tool_allowlist, "--allowed-tools only pre-approves");
}

#[test]
fn help_that_says_nothing_probes_to_nothing() {
    let caps = QoderDriver::capabilities_from_help("");
    assert!(!caps.headless && !caps.stream_json && !caps.resume);
    assert!(!caps.system_append && !caps.structured_output && !caps.tool_allowlist);
}

#[test]
fn argv_for_a_plain_read_only_job() {
    let args = build_args(&job("write a chapter"), None, &recorded_caps());
    assert_eq!(
        args,
        [
            "-p",
            "--output-format",
            "stream-json",
            "--permission-mode",
            "dont_ask",
            "--",
            "write a chapter"
        ]
    );
}

#[test]
fn argv_starts_with_a_flag_so_the_qoder_dispatcher_picks_the_cli() {
    // The `qoder` entry script sends a first argument that is an existing path
    // to the IDE. Only a leading `-` guarantees the CLI.
    let args = build_args(&job("/tmp"), None, &recorded_caps());
    assert!(args[0].starts_with('-'));
    assert_eq!(args.last().map(String::as_str), Some("/tmp"));
}

#[test]
fn a_prompt_that_looks_like_a_flag_stays_a_prompt() {
    let args = build_args(
        &job("--dangerously-skip-permissions"),
        None,
        &recorded_caps(),
    );
    let sep = args.iter().position(|a| a == "--").expect("separator");
    assert_eq!(args[sep + 1], "--dangerously-skip-permissions");
    assert_eq!(args.len(), sep + 2);
}

#[test]
fn a_writable_job_uses_accept_edits() {
    let mut j = job("edit the file");
    j.read_only = false;
    let args = build_args(&j, None, &recorded_caps());
    let pos = args.iter().position(|a| a == "--permission-mode").unwrap();
    assert_eq!(args[pos + 1], "accept_edits");
}

#[test]
fn system_text_uses_the_native_flag_when_help_advertises_it() {
    let mut j = job("write");
    j.system_append = Some("You are a textbook author.".into());
    let args = build_args(&j, None, &recorded_caps());
    let pos = args
        .iter()
        .position(|a| a == "--append-system-prompt")
        .unwrap();
    assert_eq!(args[pos + 1], "You are a textbook author.");
    assert_eq!(args.last().map(String::as_str), Some("write"));
}

#[test]
fn system_text_moves_into_the_prompt_when_help_lacks_the_flag() {
    let kept: Vec<&str> = HELP
        .lines()
        .filter(|l| !l.contains("--append-system-prompt"))
        .collect();
    let caps = QoderDriver::capabilities_from_help(&kept.join("\n"));
    let mut j = job("write");
    j.system_append = Some("Be brief.".into());
    let args = build_args(&j, None, &caps);
    assert!(!args.iter().any(|a| a == "--append-system-prompt"));
    assert_eq!(
        args.last().map(String::as_str),
        Some("Instructions for this task (follow them for the whole reply):\nBe brief.\n\nTask:\nwrite")
    );
}

#[test]
fn a_schema_is_prompt_enforced_because_help_has_no_json_schema() {
    let mut j = job("answer");
    j.json_schema = Some(json!({"type":"integer"}));
    let args = build_args(&j, None, &recorded_caps());
    assert!(!args.iter().any(|a| a == "--json-schema"));
    assert_eq!(
        args.last().map(String::as_str),
        Some(
            "answer\n\nReply with one JSON value that conforms to this JSON Schema and nothing else: no prose and no Markdown code fence.\nJSON Schema: {\"type\":\"integer\"}"
        )
    );
}

#[test]
fn a_future_help_that_lists_json_schema_gets_the_native_flag() {
    let caps = QoderDriver::capabilities_from_help(&format!(
        "{HELP}\n  --json-schema <schema>   JSON Schema for structured output\n"
    ));
    assert!(caps.structured_output);
    let mut j = job("answer");
    j.json_schema = Some(json!({"type":"integer"}));
    let args = build_args(&j, None, &caps);
    let pos = args.iter().position(|a| a == "--json-schema").unwrap();
    assert_eq!(args[pos + 1], "{\"type\":\"integer\"}");
    assert_eq!(args.last().map(String::as_str), Some("answer"));
}

#[test]
fn resume_passes_the_session_id_before_the_separator() {
    let args = build_args(&job("continue"), Some("sess-xyz"), &recorded_caps());
    let pos = args.iter().position(|a| a == "--resume").unwrap();
    assert_eq!(args[pos + 1], "sess-xyz");
    let sep = args.iter().position(|a| a == "--").unwrap();
    assert!(pos < sep);
}
