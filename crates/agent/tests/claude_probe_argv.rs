//! Claude seams that need no subprocess: the capability probe read from the
//! recorded claude 2.1.319 `--help`, and argv as a pure function of the job.

use tutordocs_agent::claude::{build_args, ClaudeDriver};
use tutordocs_agent::{Capabilities, Effort, JobSpec, ModelId};

const HELP: &str = include_str!("fixtures/claude/help.txt");

fn recorded_caps() -> Capabilities {
    ClaudeDriver::capabilities_from_help(HELP)
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let pos = args.iter().position(|a| a == flag)?;
    args.get(pos + 1).map(String::as_str)
}

fn fetch_job() -> JobSpec {
    let mut job = JobSpec::new("fetch", "/staging/job-1");
    job.read_only = false;
    job.allowed_tools = Some(vec!["WebFetch".into(), "Write".into()]);
    job
}

#[test]
fn recorded_help_advertises_a_tool_allowlist_and_a_read_only_mode() {
    let caps = recorded_caps();
    assert!(
        caps.tool_allowlist,
        "--tools and --allowedTools are in the help"
    );
    assert!(caps.read_only, "--permission-mode lists plan");
}

#[test]
fn help_without_the_tools_flag_cannot_enforce_an_allowlist() {
    let kept: Vec<&str> = HELP.lines().filter(|l| !l.contains("--tools ")).collect();
    let caps = ClaudeDriver::capabilities_from_help(&kept.join("\n"));
    assert!(!caps.tool_allowlist, "--allowedTools only pre-approves");
    assert!(caps.read_only);
}

#[test]
fn a_job_without_an_allowlist_passes_no_tool_flags() {
    let args = build_args(&JobSpec::new("hi", "/w"), None, &recorded_caps());
    assert!(!args.iter().any(|a| a == "--tools" || a == "--allowedTools"));
    assert_eq!(value_after(&args, "--permission-mode"), Some("plan"));
}

#[test]
fn an_allowlist_limits_the_tool_set_and_pre_approves_only_non_file_tools() {
    let args = build_args(&fetch_job(), None, &recorded_caps());
    assert_eq!(value_after(&args, "--tools"), Some("WebFetch,Write"));
    // A bare Write allow rule approves a write anywhere; acceptEdits approves
    // writes inside the working directory only, so Write is left to the mode.
    assert_eq!(value_after(&args, "--allowedTools"), Some("WebFetch"));
    assert_eq!(value_after(&args, "--permission-mode"), Some("acceptEdits"));
}

#[test]
fn an_allowlist_of_only_file_tools_pre_approves_nothing() {
    let mut job = fetch_job();
    job.allowed_tools = Some(vec!["Write".into(), "Edit".into()]);
    let args = build_args(&job, None, &recorded_caps());
    assert_eq!(value_after(&args, "--tools"), Some("Write,Edit"));
    assert!(!args.iter().any(|a| a == "--allowedTools"));
}

#[test]
fn an_unset_model_and_effort_pass_no_selection_flags() {
    let caps = recorded_caps();
    assert!(
        caps.model && caps.effort,
        "the recorded help lists both flags"
    );
    let args = build_args(&JobSpec::new("hi", "/w"), None, &caps);
    assert!(!args.iter().any(|a| a == "--model" || a == "--effort"));
}

#[test]
fn a_set_model_and_effort_are_passed_when_help_advertises_them() {
    let mut job = JobSpec::new("hi", "/w");
    job.model = Some(ModelId::parse("sonnet").unwrap());
    job.effort = Some(Effort::parse("low").unwrap());
    let args = build_args(&job, None, &recorded_caps());
    assert_eq!(value_after(&args, "--model"), Some("sonnet"));
    assert_eq!(value_after(&args, "--effort"), Some("low"));
}

#[test]
fn a_set_model_and_effort_are_omitted_when_help_does_not_advertise_them() {
    let mut caps = recorded_caps();
    caps.model = false;
    caps.effort = false;
    let mut job = JobSpec::new("hi", "/w");
    job.model = Some(ModelId::parse("sonnet").unwrap());
    job.effort = Some(Effort::parse("low").unwrap());
    let args = build_args(&job, None, &caps);
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
    let caps = ClaudeDriver::capabilities_from_help(&kept.join("\n"));
    assert!(!caps.model);
    assert!(!caps.effort);
    assert!(caps.read_only, "dropping selection flags leaves plan mode");
}

#[test]
fn only_a_job_limited_to_non_editing_tools_skips_prompts_and_every_job_drops_hooks_and_mcp() {
    // No allowlist leaves the CLI's whole default tool set, which can edit
    // anywhere under bypassPermissions, so it keeps the permission check.
    let args = build_args(&JobSpec::new("hi", "/w"), None, &recorded_caps());
    assert!(!args.iter().any(|a| a == "--dangerously-skip-permissions"));
    let mut read_only = JobSpec::new("hi", "/w");
    read_only.allowed_tools = Some(vec![
        "Read".to_string(),
        "Grep".to_string(),
        "Glob".to_string(),
    ]);
    let reading = build_args(&read_only, None, &recorded_caps());
    assert!(reading
        .iter()
        .any(|a| a == "--dangerously-skip-permissions"));
    assert_eq!(value_after(&reading, "--permission-mode"), Some("plan"));
    assert!(args.iter().any(|a| a == "--strict-mcp-config"));
    assert!(!args.iter().any(|a| a == "--mcp-config"));
    assert_eq!(
        value_after(&args, "--settings"),
        Some("{\"disableAllHooks\":true}")
    );
    assert!(args.iter().any(|a| a == "--include-hook-events"));
    assert_eq!(value_after(&args, "--permission-mode"), Some("plan"));
    assert!(!args.iter().any(|a| a == "--tools" || a == "--allowedTools"));

    let allowed = build_args(&fetch_job(), None, &recorded_caps());
    assert_eq!(value_after(&allowed, "--tools"), Some("WebFetch,Write"));
    assert!(!allowed
        .iter()
        .any(|a| a == "--dangerously-skip-permissions"));
    assert_eq!(
        value_after(&allowed, "--permission-mode"),
        Some("acceptEdits")
    );
    assert!(allowed.iter().any(|a| a == "--strict-mcp-config"));
    assert!(!allowed.iter().any(|a| a == "--mcp-config"));
    assert_eq!(
        value_after(&allowed, "--settings"),
        Some("{\"disableAllHooks\":true}")
    );
}

#[test]
fn a_file_editing_allowlist_uses_accept_edits_without_the_skip_flag() {
    for tools in [
        vec!["Write"],
        vec!["Edit"],
        vec!["MultiEdit"],
        vec!["NotebookEdit"],
        vec!["Read", "Grep", "Glob", "Write"],
        vec!["WebFetch", "Write"],
    ] {
        let mut job = JobSpec::new("write", "/w");
        job.read_only = false;
        job.allowed_tools = Some(tools.iter().map(|tool| (*tool).to_string()).collect());
        let args = build_args(&job, None, &recorded_caps());
        let label = tools.join(",");
        assert!(
            !args
                .iter()
                .any(|arg| arg == "--dangerously-skip-permissions"),
            "{label}"
        );
        assert_eq!(
            value_after(&args, "--permission-mode"),
            Some("acceptEdits"),
            "{label}"
        );
        assert!(
            args.iter().any(|arg| arg == "--strict-mcp-config"),
            "{label}"
        );
        assert_eq!(
            value_after(&args, "--settings"),
            Some("{\"disableAllHooks\":true}"),
            "{label}"
        );
    }
}

#[test]
fn an_allowlist_with_no_file_editing_tool_keeps_the_skip_flag() {
    let mut job = JobSpec::new("read", "/w");
    job.read_only = true;
    job.allowed_tools = Some(vec!["Read".into(), "Grep".into(), "Glob".into()]);
    let args = build_args(&job, None, &recorded_caps());
    assert_eq!(
        args,
        vec![
            "-p",
            "read",
            "--output-format",
            "stream-json",
            "--verbose",
            "--dangerously-skip-permissions",
            "--strict-mcp-config",
            "--settings",
            "{\"disableAllHooks\":true}",
            "--include-hook-events",
            "--permission-mode",
            "plan",
            "--tools",
            "Read,Grep,Glob",
            "--allowedTools",
            "Read,Grep,Glob",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>()
    );
}

#[test]
fn an_empty_allowlist_disables_every_tool() {
    let mut job = fetch_job();
    job.allowed_tools = Some(Vec::new());
    let args = build_args(&job, None, &recorded_caps());
    assert_eq!(value_after(&args, "--tools"), Some(""));
    assert!(!args.iter().any(|a| a == "--allowedTools"));
}
