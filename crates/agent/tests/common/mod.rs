//! Shared fake-claude installer. Each test bakes its own steering into the
//! script it installs, so no test mutates process env and tests run in
//! parallel without interfering.
#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tutordocs_agent::{AgentEvent, EventStream};

const FAKE_SCRIPT: &str = include_str!("../fixtures/fake-claude.sh");

/// Write the fake CLI into `dir` with `vars` assigned at the top of the
/// script. The assignments are shell-local, not exported, so they never show
/// up in the child env a test dumps.
pub fn install_fake(dir: &Path, vars: &[(&str, &str)]) -> PathBuf {
    let mut header = String::from("#!/bin/sh\n");
    for (name, value) in vars {
        assert!(!value.contains('\''), "fake var {name} cannot hold a quote");
        header.push_str(&format!("{name}='{value}'\n"));
    }
    let body = FAKE_SCRIPT
        .strip_prefix("#!/bin/sh\n")
        .expect("fake script starts with a shebang");
    let path = dir.join("fake-claude");
    std::fs::write(&path, format!("{header}{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

pub async fn collect(stream: &mut EventStream) -> Vec<AgentEvent> {
    let mut out = Vec::new();
    while let Some(ev) = stream.recv().await {
        let done = ev == AgentEvent::Done;
        out.push(ev);
        if done {
            break;
        }
    }
    out
}

pub async fn collect_bounded(stream: &mut EventStream) -> Vec<AgentEvent> {
    tokio::time::timeout(Duration::from_secs(5), collect(stream))
        .await
        .expect("job finished within 5s")
}

const FAKE_AGENT_SCRIPT: &str = include_str!("../fixtures/fake-agent.sh");

/// What a fake qoder or agy prints for `--help` and for a job.
pub struct FakeAgent<'a> {
    pub version: &'a str,
    pub help: &'a str,
    /// agy prints its help on stderr; qoder on stdout.
    pub help_on_stderr: bool,
    pub stream: &'a str,
}

/// Write the fake qoder/agy CLI into `dir` with `vars` assigned at the top of
/// the script, shell-local so they never appear in a dumped child env.
pub fn install_agent(dir: &Path, fake: &FakeAgent, vars: &[(&str, &str)]) -> PathBuf {
    std::fs::write(dir.join("help.txt"), fake.help).unwrap();
    std::fs::write(dir.join("stream.ndjson"), fake.stream).unwrap();
    let mut header = String::from("#!/bin/sh\n");
    let mut assign = |name: &str, value: &str| {
        assert!(!value.contains('\''), "fake var {name} cannot hold a quote");
        header.push_str(&format!("{name}='{value}'\n"));
    };
    assign("FAKE_DIR", dir.to_str().expect("utf-8 temp path"));
    assign("FAKE_VERSION", fake.version);
    assign(
        "FAKE_HELP_STREAM",
        if fake.help_on_stderr { "err" } else { "out" },
    );
    for (name, value) in vars {
        assign(name, value);
    }
    let body = FAKE_AGENT_SCRIPT
        .strip_prefix("#!/bin/sh\n")
        .expect("fake script starts with a shebang");
    let path = dir.join("fake-agent");
    std::fs::write(&path, format!("{header}{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A `sandbox-exec` stand-in. It copies the profile argument when asked,
/// records `$0` and every argument, then execs the wrapped command. That
/// command inherits this process's environment, which is the scrubbed one.
pub fn passthrough_sandbox(
    dir: &Path,
    argv_dump: Option<&Path>,
    profile_copy: Option<&Path>,
) -> PathBuf {
    let mut body = String::from("#!/bin/sh\n");
    if let Some(copy) = profile_copy {
        let copy = copy.to_str().expect("utf-8 profile copy path");
        assert!(
            !copy.contains('\''),
            "profile copy path cannot hold a quote"
        );
        body.push_str(&format!("cp \"$2\" '{copy}'\n"));
    }
    if let Some(dump) = argv_dump {
        let dump = dump.to_str().expect("utf-8 argv dump path");
        assert!(!dump.contains('\''), "argv dump path cannot hold a quote");
        body.push_str(&format!("printf '%s\\0' \"$0\" \"$@\" > '{dump}'\n"));
    }
    body.push_str("shift 2\nexec \"$@\"\n");
    let path = dir.join("sandbox-exec");
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// The arguments the fake received, split on the NUL bytes it wrote.
pub fn read_argv(dump: &Path) -> Vec<String> {
    std::fs::read_to_string(dump)
        .unwrap()
        .split('\0')
        .filter(|a| !a.is_empty())
        .map(str::to_string)
        .collect()
}
