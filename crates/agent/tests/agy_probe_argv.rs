//! agy seams that need no subprocess: the capability probe read from the
//! recorded `--help` (which agy prints on stderr), and argv as a pure
//! function of the job.

use serde_json::json;
use tutordocs_agent::agy::{build_args, AgyDriver};
use tutordocs_agent::{AuthKind, Capabilities, Effort, JobSpec, ModelId};

const HELP: &str = include_str!("fixtures/agy/help.stderr.txt");

fn recorded_caps() -> Capabilities {
    AgyDriver::capabilities_from_help(HELP)
}

fn job(prompt: &str) -> JobSpec {
    JobSpec::new(prompt, "/work")
}

#[test]
fn recorded_help_probes_to_the_documented_capabilities() {
    let caps = recorded_caps();
    assert!(caps.headless && caps.stream_json);
    assert!(caps.structured_output, "--json-schema is in the help");
    assert!(caps.resume, "--conversation is in the help");
    assert!(
        !caps.system_append,
        "agy has no --append-system-prompt; system text goes in the prompt"
    );
    assert!(
        !caps.tool_allowlist,
        "agy takes tool rules from settings.json"
    );
    assert_eq!(caps.auth, AuthKind::SubscriptionOAuth);
}

#[test]
fn a_flag_dropped_by_an_update_degrades_only_its_capability() {
    let without = |flag: &str| -> Capabilities {
        let kept: Vec<&str> = HELP.lines().filter(|l| !l.contains(flag)).collect();
        AgyDriver::capabilities_from_help(&kept.join("\n"))
    };
    let caps = without("--json-schema");
    assert!(!caps.structured_output);
    assert!(caps.headless && caps.resume && caps.stream_json);

    let caps = without("--conversation");
    assert!(!caps.resume);
    assert!(caps.headless && caps.structured_output);

    let caps = without("--print");
    assert!(!caps.headless, "no --print anywhere means no headless mode");
}

#[test]
fn help_that_says_nothing_probes_to_nothing() {
    let caps = AgyDriver::capabilities_from_help("");
    assert!(!caps.headless && !caps.stream_json && !caps.resume && !caps.structured_output);
}

#[test]
fn agy_states_it_can_neither_run_read_only_nor_restrict_tools() {
    // The ticket 11 real job passed `--mode plan` and agy's init reported
    // `permission_mode: "request-review"`, and the help has no tool flag.
    let caps = recorded_caps();
    assert!(!caps.read_only);
    assert!(!caps.tool_allowlist);
}

#[test]
fn argv_never_claims_plan_mode() {
    let mut j = job("write a chapter");
    j.read_only = false;
    let args = build_args(&j, None, &recorded_caps());
    assert_eq!(
        args,
        [
            "-p",
            "write a chapter",
            "--output-format",
            "stream-json",
            "--dangerously-skip-permissions",
        ]
    );
    assert!(!args.iter().any(|a| a == "--mode" || a == "accept-edits"));
}

#[test]
fn the_prompt_is_the_value_of_dash_p_even_when_it_looks_like_a_flag() {
    let args = build_args(
        &job("--dangerously-skip-permissions"),
        None,
        &recorded_caps(),
    );
    assert_eq!(args[0], "-p");
    assert_eq!(args[1], "--dangerously-skip-permissions");
}

#[test]
fn a_job_skips_permission_prompts_instead_of_passing_accept_edits() {
    let mut j = job("edit the file");
    j.read_only = false;
    let args = build_args(&j, None, &recorded_caps());
    assert!(args.iter().any(|a| a == "--dangerously-skip-permissions"));
    assert!(!args.iter().any(|a| a == "--mode" || a == "accept-edits"));
}

#[test]
fn the_job_timeout_is_the_pump_kill_not_a_cli_flag() {
    // A hung agy would not honor its own --print-timeout, so the driver does
    // not rely on it and passes no limit to the CLI.
    let mut j = job("hi");
    j.timeout = Some(std::time::Duration::from_secs(5));
    let args = build_args(&j, None, &recorded_caps());
    assert!(!args.iter().any(|a| a == "--print-timeout"));
}

#[test]
fn system_text_goes_through_the_prompt_body() {
    let mut j = job("write");
    j.system_append = Some("Be brief.".into());
    let args = build_args(&j, None, &recorded_caps());
    assert!(!args.iter().any(|a| a.contains("system-prompt")));
    assert_eq!(
        args[1],
        "Instructions for this task (follow them for the whole reply):\nBe brief.\n\nTask:\nwrite"
    );
}

#[test]
fn a_schema_uses_the_native_flag_when_help_advertises_it() {
    let mut j = job("answer");
    j.json_schema = Some(json!({"type":"integer"}));
    let args = build_args(&j, None, &recorded_caps());
    let pos = args.iter().position(|a| a == "--json-schema").unwrap();
    assert_eq!(args[pos + 1], "{\"type\":\"integer\"}");
    assert_eq!(args[1], "answer", "the prompt is untouched");
}

#[test]
fn a_schema_is_prompt_enforced_when_an_update_drops_the_flag() {
    let kept: Vec<&str> = HELP
        .lines()
        .filter(|l| !l.contains("--json-schema"))
        .collect();
    let caps = AgyDriver::capabilities_from_help(&kept.join("\n"));
    let mut j = job("answer");
    j.json_schema = Some(json!({"type":"integer"}));
    let args = build_args(&j, None, &caps);
    assert!(!args.iter().any(|a| a == "--json-schema"));
    assert!(args[1].starts_with("answer\n\nReply with one JSON value"));
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let pos = args.iter().position(|a| a == name)?;
    args.get(pos + 1).map(String::as_str)
}

#[test]
fn an_unset_model_and_effort_pass_no_selection_flags() {
    let caps = recorded_caps();
    assert!(
        caps.model && caps.effort,
        "the recorded help lists both flags"
    );
    let args = build_args(&job("hi"), None, &caps);
    assert!(!args.iter().any(|a| a == "--model" || a == "--effort"));
}

#[test]
fn a_set_model_and_effort_are_passed_when_help_advertises_them() {
    let mut j = job("hi");
    j.model = Some(ModelId::parse("gemini-3.8-flash").unwrap());
    j.effort = Some(Effort::parse("xhigh").unwrap());
    let args = build_args(&j, None, &recorded_caps());
    assert_eq!(flag(&args, "--model"), Some("gemini-3.8-flash"));
    assert_eq!(flag(&args, "--effort"), Some("xhigh"));
}

#[test]
fn a_set_model_and_effort_are_omitted_when_help_does_not_advertise_them() {
    let mut caps = recorded_caps();
    caps.model = false;
    caps.effort = false;
    let mut j = job("hi");
    j.model = Some(ModelId::parse("gemini-3.8-flash").unwrap());
    j.effort = Some(Effort::parse("xhigh").unwrap());
    let args = build_args(&j, None, &caps);
    assert!(!args.iter().any(|a| a == "--model" || a == "--effort"));
}

#[test]
fn help_without_the_selection_flags_does_not_advertise_them() {
    let kept: Vec<&str> = HELP
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("--model") && !line.starts_with("--effort")
        })
        .collect();
    let caps = AgyDriver::capabilities_from_help(&kept.join("\n"));
    assert!(!caps.model && !caps.effort);
    assert!(caps.headless);
}

#[test]
fn resume_passes_the_conversation_id() {
    let args = build_args(&job("continue"), Some("conv-xyz"), &recorded_caps());
    let pos = args.iter().position(|a| a == "--conversation").unwrap();
    assert_eq!(args[pos + 1], "conv-xyz");
}
