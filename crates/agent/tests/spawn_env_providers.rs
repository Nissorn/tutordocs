//! The spawned Qoder and agy children really lack their provider's metering
//! variables and Claude's, with the parent holding all of them. It mutates the
//! real parent process env, so it lives alone in its own test binary and runs
//! its providers one after the other.

use tutordocs_agent::agy::AgyDriver;
use tutordocs_agent::env::{AGY_METERING_ENV_VARS, METERING_ENV_VARS, QODER_METERING_ENV_VARS};
use tutordocs_agent::qoder::QoderDriver;
use tutordocs_agent::{AgentDriver, JobSpec};

mod common;
use common::{collect_bounded, install_agent, passthrough_sandbox, FakeAgent};

const QODER_HELP: &str = include_str!("fixtures/qoder/help.txt");
const AGY_HELP: &str = include_str!("fixtures/agy/help.stderr.txt");
const QODER_STREAM: &str = include_str!("fixtures/qoder/stream-from-schema.ndjson");
const AGY_STREAM: &str = include_str!("fixtures/agy/stream-from-docs.ndjson");

fn dumped_env(
    rt: &tokio::runtime::Runtime,
    dir: &std::path::Path,
    help: &str,
    stderr: bool,
    stream: &str,
    make: impl FnOnce(std::path::PathBuf) -> Box<dyn AgentDriver>,
) -> String {
    let dump = dir.join("child-env.txt");
    let fake = FakeAgent {
        version: "1.0.0",
        help,
        help_on_stderr: stderr,
        stream,
    };
    let bin = install_agent(dir, &fake, &[("FAKE_ENV_DUMP", dump.to_str().unwrap())]);
    let driver = make(bin);
    rt.block_on(async {
        let mut job = JobSpec::new("hi", dir);
        job.read_only = false;
        let h = driver.start(job).await.unwrap();
        let _ = collect_bounded(&mut driver.events(&h)).await;
    });
    std::fs::read_to_string(&dump).unwrap()
}

#[test]
fn spawned_children_really_lack_every_metering_var_of_their_provider() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let all: Vec<&str> = METERING_ENV_VARS
        .iter()
        .chain(QODER_METERING_ENV_VARS)
        .chain(AGY_METERING_ENV_VARS)
        .copied()
        .collect();

    // Fabricated dummy literals, not credentials; never printed.
    for name in &all {
        std::env::set_var(name, "dummy-not-a-key");
    }
    std::env::set_var("TUTORDOCS_SPAWN_ENV_CANARY", "kept");
    std::env::set_var("QODER_CONFIG_DIR", "/dummy/qoder-config");

    let qoder_dir = tmp.path().join("qoder");
    let agy_dir = tmp.path().join("agy");
    std::fs::create_dir(&qoder_dir).unwrap();
    std::fs::create_dir(&agy_dir).unwrap();
    let qoder_env = dumped_env(&rt, &qoder_dir, QODER_HELP, false, QODER_STREAM, |bin| {
        Box::new(QoderDriver::new(bin).unwrap())
    });
    let agy_env = dumped_env(&rt, &agy_dir, AGY_HELP, true, AGY_STREAM, |bin| {
        let sandbox = passthrough_sandbox(&agy_dir, None, None);
        Box::new(AgyDriver::new(bin).unwrap().with_sandbox_exec(sandbox))
    });

    for name in &all {
        std::env::remove_var(name);
    }
    std::env::remove_var("TUTORDOCS_SPAWN_ENV_CANARY");
    std::env::remove_var("QODER_CONFIG_DIR");

    for (who, env, own) in [
        ("qoder", &qoder_env, QODER_METERING_ENV_VARS),
        ("agy", &agy_env, AGY_METERING_ENV_VARS),
    ] {
        assert!(
            env.lines().any(|l| l == "TUTORDOCS_SPAWN_ENV_CANARY=kept"),
            "{who} lost ordinary env too, so the scrub proof would be vacuous"
        );
        for name in METERING_ENV_VARS.iter().chain(own) {
            assert!(
                !env.lines().any(|l| l.starts_with(&format!("{name}="))),
                "the spawned {who} child saw {name}"
            );
        }
    }
    assert!(
        qoder_env
            .lines()
            .any(|l| l == "QODER_CONFIG_DIR=/dummy/qoder-config"),
        "qoder needs QODER_CONFIG_DIR to find its saved login"
    );
}
