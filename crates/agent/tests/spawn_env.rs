//! The one test that must set a metering variable in the real parent process
//! and look at what the spawned child actually received. It lives alone in its
//! own test binary, so no sibling test can see or race the mutation.

use tutordocs_agent::claude::ClaudeDriver;
use tutordocs_agent::env::METERING_ENV_VARS;
use tutordocs_agent::{AgentDriver, JobSpec};

mod common;
use common::{collect_bounded, install_fake};

#[tokio::test]
async fn spawned_child_env_really_lacks_every_metering_var() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("child-env.txt");
    let fake = install_fake(
        tmp.path(),
        &[("FAKE_CLAUDE_ENV_DUMP", dump.to_str().unwrap())],
    );
    let d = ClaudeDriver::new(fake).expect("fake CLI probes cleanly");

    // Fabricated dummy literals, not credentials; never printed.
    for name in METERING_ENV_VARS {
        std::env::set_var(name, "dummy-not-a-key");
    }
    std::env::set_var("TUTORDOCS_SPAWN_ENV_CANARY", "kept");
    let h = d.start(JobSpec::new("hi", tmp.path())).await.unwrap();
    let _ = collect_bounded(&mut d.events(&h)).await;
    for name in METERING_ENV_VARS {
        std::env::remove_var(name);
    }
    std::env::remove_var("TUTORDOCS_SPAWN_ENV_CANARY");

    let contents = std::fs::read_to_string(&dump).unwrap();
    assert!(
        contents
            .lines()
            .any(|l| l == "TUTORDOCS_SPAWN_ENV_CANARY=kept"),
        "the child lost ordinary env too, so the scrub proof would be vacuous"
    );
    for name in METERING_ENV_VARS {
        assert!(
            !contents.lines().any(|l| l.starts_with(&format!("{name}="))),
            "the spawned child saw {name}"
        );
    }
}
