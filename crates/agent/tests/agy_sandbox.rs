//! agy's guard is an OS sandbox around the child, not an agy flag.
//!
//! The profile text is a hand substitution of `crates/agent/src/agy.sb`. The
//! argv is the process the driver spawns. A missing `sandbox-exec` refuses
//! the job before the CLI starts.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tutordocs_agent::agy::{build_args, render_profile, sandbox_argv, AgyDriver};
use tutordocs_agent::{AgentDriver, JobSpec};

const HELP: &str = include_str!("fixtures/agy/help.stderr.txt");

fn caps() -> tutordocs_agent::Capabilities {
    AgyDriver::capabilities_from_help(HELP)
}

#[test]
fn the_profile_fills_home_and_job_and_uses_the_resolved_agy_binary() {
    let profile = render_profile("/Users/ada", "/Users/ada/course/job", "/opt/bin/agy")
        .expect("plain paths are safe");
    let expected = "\
(version 1)
(allow default)
(deny file-write* (subpath \"/Users\") (subpath \"/private/tmp\") (subpath \"/private/var/tmp\"))
(deny file-read* (subpath \"/Users/ada\"))
(allow file-read* file-write*
  (subpath \"/Users/ada/course/job\")
  (subpath \"/Users/ada/.gemini\")
  (subpath \"/Users/ada/Library/Application Support/Antigravity\")
  (subpath \"/Users/ada/Library/Caches/antigravity\")
  (subpath \"/Users/ada/Library/Caches/com.google.antigravity\"))
(allow file-read*
  (literal \"/Users/ada\")
  (subpath \"/Users/ada/.local/bin\")
  (subpath \"/Users/ada/Library/Keychains\")
  (subpath \"/Users/ada/Library/Preferences\"))
(deny process-exec)
(allow process-exec (literal \"/opt/bin/agy\"))
(allow process-exec (literal \"/usr/bin/security\"))
(deny file-read* (literal \"/Users/ada/.gemini/config/hooks.json\") (literal \"/Users/ada/.gemini/config/hooks.json.bak\"))
";
    assert_eq!(profile, expected);
    assert!(
        !profile.contains("/Users/ada/.local/bin/agy"),
        "the exec literal must be the resolved binary, not the template path"
    );
}

#[test]
fn the_sandbox_lets_agy_run_the_keychain_helper() {
    let sandbox_exec = Path::new("/usr/bin/sandbox-exec");
    if !sandbox_exec.exists() {
        eprintln!("skip: /usr/bin/sandbox-exec is missing, the driver refuses every agy job here");
        return;
    }
    let home = std::env::var("HOME")
        .expect("HOME is set")
        .trim_end_matches('/')
        .to_string();
    let installed = PathBuf::from(&home).join(".local/bin/agy");
    let agy = std::fs::canonicalize(&installed).unwrap_or(installed);
    let tmp = tempfile::tempdir().unwrap();
    let job = tmp.path().join("job");
    std::fs::create_dir(&job).unwrap();
    let profile = render_profile(&home, job.to_str().unwrap(), agy.to_str().unwrap())
        .expect("real paths are safe");
    let profile_file = tmp.path().join("agy.sb");
    std::fs::write(&profile_file, profile).unwrap();

    let out = Command::new(sandbox_exec)
        .arg("-f")
        .arg(&profile_file)
        .arg("/usr/bin/security")
        .arg("help")
        .stdin(Stdio::null())
        .output()
        .expect("sandbox-exec runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("list-keychains"),
        "agy reaches its keychain login only if security executes, got: {text}"
    );
    assert!(
        !text.contains("Operation not permitted"),
        "the profile must not deny the keychain helper, got: {text}"
    );
}

#[test]
fn a_job_path_with_a_quote_or_a_backslash_is_refused() {
    let quote = render_profile("/Users/ada", "/Users/ada/job\"x", "/opt/bin/agy").unwrap_err();
    assert!(
        quote.to_string().contains("quote") || quote.to_string().contains("backslash"),
        "{quote}"
    );
    let slash = render_profile("/Users/ada", "/Users/ada/job\\x", "/opt/bin/agy").unwrap_err();
    assert!(
        slash.to_string().contains("quote") || slash.to_string().contains("backslash"),
        "{slash}"
    );
    let binary = render_profile("/Users/ada", "/Users/ada/job", "/opt/bin/agy\"bad").unwrap_err();
    assert!(
        binary.to_string().contains("quote") || binary.to_string().contains("backslash"),
        "{binary}"
    );
}

#[test]
fn the_child_is_sandbox_exec_then_agy_without_accept_edits() {
    let job = JobSpec::new("hello", "/work");
    let agy_args = build_args(&job, None, &caps());
    let argv = sandbox_argv(
        "/usr/bin/sandbox-exec",
        "/tmp/profile.sb",
        "/opt/bin/agy",
        &agy_args,
    );
    assert_eq!(
        argv,
        [
            "/usr/bin/sandbox-exec",
            "-f",
            "/tmp/profile.sb",
            "/opt/bin/agy",
            "-p",
            "hello",
            "--output-format",
            "stream-json",
            "--dangerously-skip-permissions",
        ]
    );
}

mod common;
use common::{collect_bounded, install_agent, passthrough_sandbox, read_argv, FakeAgent};

const STREAM: &str = include_str!("fixtures/agy/stream-from-docs.ndjson");

#[tokio::test]
async fn a_missing_sandbox_refuses_before_the_cli_is_spawned() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("argv");
    let fake = FakeAgent {
        version: "1.5.1",
        help: HELP,
        help_on_stderr: true,
        stream: STREAM,
    };
    let bin = install_agent(
        tmp.path(),
        &fake,
        &[("FAKE_ARGV_DUMP", dump.to_str().unwrap())],
    );
    let driver = AgyDriver::new(bin)
        .unwrap()
        .with_sandbox_exec(tmp.path().join("no-sandbox-exec"));
    assert!(!driver.capabilities().read_only);
    assert!(!driver.capabilities().tool_allowlist);

    let started = driver.start(JobSpec::new("hi", tmp.path())).await;
    let err = match started {
        Err(err) => err,
        Ok(_) => panic!("a missing sandbox must refuse the job"),
    };
    assert!(
        err.to_string().contains("missing"),
        "a read-only course job keeps a plain refusal: {err}"
    );
    assert!(!dump.exists(), "the CLI must not be spawned");
}

#[tokio::test]
async fn a_sandboxed_job_writes_the_profile_outside_the_workdir_and_wraps_agy() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("job");
    std::fs::create_dir(&work).unwrap();
    let dump = tmp.path().join("sandbox-argv");
    let saved = tmp.path().join("profile-copy.sb");
    let fake = FakeAgent {
        version: "1.5.1",
        help: HELP,
        help_on_stderr: true,
        stream: STREAM,
    };
    let bin = install_agent(
        tmp.path(),
        &fake,
        &[(
            "FAKE_ARGV_DUMP",
            tmp.path().join("agy-argv").to_str().unwrap(),
        )],
    );
    let sandbox = passthrough_sandbox(tmp.path(), Some(&dump), Some(&saved));
    let driver = AgyDriver::new(&bin).unwrap().with_sandbox_exec(&sandbox);
    assert!(driver.capabilities().read_only);
    assert!(driver.capabilities().tool_allowlist);

    let mut job = JobSpec::new("hello", &work);
    job.allowed_tools = Some(vec!["Read".into(), "Grep".into(), "Glob".into()]);
    let handle = driver.start(job).await.expect("sandbox present, job runs");
    let _events = collect_bounded(&mut driver.events(&handle)).await;

    let argv = read_argv(&dump);
    assert_eq!(argv[1], "-f");
    let profile = Path::new(&argv[2]);
    assert!(
        !profile.starts_with(&work),
        "profile {} must not live in the job folder",
        profile.display()
    );
    assert!(
        !profile.exists(),
        "the profile dir is removed when the job ends"
    );
    let agy_bin = std::fs::canonicalize(&bin).unwrap();
    let job_dir = std::fs::canonicalize(&work).unwrap();
    assert_eq!(argv[0], sandbox.to_str().unwrap());
    assert_eq!(Path::new(&argv[3]), agy_bin.as_path());
    let home = std::env::var("HOME")
        .unwrap()
        .trim_end_matches('/')
        .to_string();
    let expected =
        render_profile(&home, job_dir.to_str().unwrap(), agy_bin.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), expected);
    let agy_argv = read_argv(&tmp.path().join("agy-argv"));
    assert_eq!(agy_argv[0], "-p");
    assert!(agy_argv
        .iter()
        .any(|a| a == "--dangerously-skip-permissions"));
    assert!(!agy_argv
        .iter()
        .any(|a| a == "--mode" || a == "accept-edits"));
}
