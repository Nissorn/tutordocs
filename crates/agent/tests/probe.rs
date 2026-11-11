//! The startup probe is bounded: a CLI that hangs on `--version` must not
//! freeze app startup.

use std::time::{Duration, Instant};

use tutordocs_agent::agy::AgyDriver;
use tutordocs_agent::probe::run_probe_within;
use tutordocs_agent::qoder::QoderDriver;

mod common;
use common::{install_agent, FakeAgent};

fn hung(dir: &std::path::Path) -> std::path::PathBuf {
    install_agent(
        dir,
        &FakeAgent {
            version: "0.0.0",
            help: "",
            help_on_stderr: false,
            stream: "",
        },
        &[("FAKE_MODE", "version_hang")],
    )
}

#[test]
fn a_probe_that_hangs_is_killed_and_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let err = run_probe_within(&hung(tmp.path()), Duration::from_millis(300)).unwrap_err();
    assert!(err.to_string().contains("timed out"), "got: {err}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_missing_binary_is_an_error_not_a_panic() {
    let missing = std::path::Path::new("/definitely/not/installed/agy");
    assert!(AgyDriver::new(missing).is_err());
    assert!(QoderDriver::new(missing).is_err());
}

#[test]
fn help_is_read_from_both_streams() {
    let tmp = tempfile::tempdir().unwrap();
    for on_stderr in [false, true] {
        let dir = tmp.path().join(on_stderr.to_string());
        std::fs::create_dir(&dir).unwrap();
        let bin = install_agent(
            &dir,
            &FakeAgent {
                version: "9.9.9\n",
                help: "  --print  run once\n",
                help_on_stderr: on_stderr,
                stream: "",
            },
            &[],
        );
        let probe = run_probe_within(&bin, Duration::from_secs(5)).unwrap();
        assert_eq!(probe.version, "9.9.9");
        assert!(probe.help.contains("--print"), "stderr={on_stderr}");
    }
}
