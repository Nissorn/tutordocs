//! Driver detection: which of claude, qoder and agy are installed here, found
//! the way a GUI app must find them (its PATH is minimal), reported as absent
//! or unusable rather than assumed. Every binary here is a fake script.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tutordocs_agent::registry::detect;

mod common;
use common::{install_agent, install_fake, FakeAgent};

const QODER_HELP: &str = include_str!("fixtures/qoder/help.txt");
const AGY_HELP: &str = include_str!("fixtures/agy/help.stderr.txt");

fn put(bin_dir: &Path, name: &str, script: &Path) {
    std::fs::create_dir_all(bin_dir).unwrap();
    // The fake finds its sidecar files through the path baked at install, so
    // a copy of the script stays valid; only the file name changes.
    let dest = bin_dir.join(name);
    std::fs::copy(script, &dest).unwrap();
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn install_all(home: &Path) {
    let bin = home.join(".local/bin");
    let scratch = home.join("scratch");
    for d in ["claude", "qoder", "agy"] {
        std::fs::create_dir_all(scratch.join(d)).unwrap();
    }
    put(&bin, "claude", &install_fake(&scratch.join("claude"), &[]));
    let q = FakeAgent {
        version: "1.1.85",
        help: QODER_HELP,
        help_on_stderr: false,
        stream: "",
    };
    put(
        &bin,
        "qodercli",
        &install_agent(&scratch.join("qoder"), &q, &[]),
    );
    let a = FakeAgent {
        version: "1.5.1",
        help: AGY_HELP,
        help_on_stderr: true,
        stream: "",
    };
    put(&bin, "agy", &install_agent(&scratch.join("agy"), &a, &[]));
}

fn minimal_path() -> OsString {
    "/usr/bin:/bin".into()
}

#[test]
fn all_three_installed_are_found_in_a_known_dir_with_a_gui_style_path() {
    let home = tempfile::tempdir().unwrap();
    install_all(home.path());
    let found = detect(home.path(), Some(&minimal_path()));
    let ids: Vec<&str> = found.iter().map(|d| d.status.id.as_str()).collect();
    assert_eq!(ids, ["claude", "qoder", "agy"]);
    for d in &found {
        assert!(d.status.available, "{}: {:?}", d.status.id, d.status.reason);
        assert!(d.status.capabilities.is_some());
        assert!(d.driver.is_some());
    }
    assert_eq!(found[1].status.version.as_deref(), Some("1.1.85"));
    assert_eq!(found[2].status.version.as_deref(), Some("1.5.1"));
    assert_eq!(found[2].driver.as_ref().unwrap().id(), "agy");
}

#[test]
fn absent_clis_are_reported_absent_never_assumed() {
    let home = tempfile::tempdir().unwrap();
    let found = detect(home.path(), Some(&minimal_path()));
    assert_eq!(
        found.len(),
        3,
        "every driver is listed so the UI can show it absent"
    );
    for d in &found {
        assert!(!d.status.available);
        assert!(d.driver.is_none() && d.status.capabilities.is_none());
        assert!(d.status.binary.is_none() && d.status.version.is_none());
        assert!(
            d.status.reason.as_deref().unwrap().contains("not found"),
            "{}: {:?}",
            d.status.id,
            d.status.reason
        );
    }
}

#[test]
fn only_the_installed_ones_are_available() {
    let home = tempfile::tempdir().unwrap();
    install_all(home.path());
    std::fs::remove_file(home.path().join(".local/bin/agy")).unwrap();
    let found = detect(home.path(), Some(&minimal_path()));
    let available: Vec<(&str, bool)> = found
        .iter()
        .map(|d| (d.status.id.as_str(), d.status.available))
        .collect();
    assert_eq!(
        available,
        [("claude", true), ("qoder", true), ("agy", false)]
    );
}

#[test]
fn a_binary_that_fails_its_probe_is_unusable_not_absent() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("agy"), "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(bin.join("agy"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let found = detect(home.path(), Some(&minimal_path()));
    let agy = found.iter().find(|d| d.status.id == "agy").unwrap();
    assert!(!agy.status.available);
    assert_eq!(
        agy.status.binary.as_deref(),
        Some(bin.join("agy").as_path())
    );
    let reason = agy.status.reason.as_deref().unwrap();
    assert!(reason.contains("--version"), "got: {reason}");
    assert!(!reason.contains("not found"));
}

#[test]
fn a_dir_on_path_wins_over_a_known_dir() {
    let home = tempfile::tempdir().unwrap();
    install_all(home.path());
    let other = home.path().join("elsewhere");
    let scratch = home.path().join("scratch/other");
    std::fs::create_dir_all(&scratch).unwrap();
    let a = FakeAgent {
        version: "9.9.9",
        help: AGY_HELP,
        help_on_stderr: true,
        stream: "",
    };
    put(&other, "agy", &install_agent(&scratch, &a, &[]));
    let path: OsString = format!("{}:/usr/bin", other.display()).into();
    let found = detect(home.path(), Some(&path));
    let agy = found.iter().find(|d| d.status.id == "agy").unwrap();
    assert_eq!(agy.status.version.as_deref(), Some("9.9.9"));
}

#[test]
fn a_detected_driver_is_ready_with_its_probed_capabilities() {
    let home = tempfile::tempdir().unwrap();
    install_all(home.path());
    let found = detect(home.path(), Some(&minimal_path()));
    let qoder = found[1].driver.as_ref().unwrap();
    assert_eq!(qoder.id(), "qoder");
    assert_eq!(
        qoder.capabilities(),
        found[1].status.capabilities.clone().unwrap()
    );
    assert!(qoder.capabilities().headless);
}

#[test]
fn the_status_serializes_to_the_shape_the_ui_will_read() {
    let home = tempfile::tempdir().unwrap();
    install_all(home.path());
    std::fs::remove_file(home.path().join(".local/bin/agy")).unwrap();
    let found = detect(home.path(), Some(&minimal_path()));
    let statuses: Vec<_> = found.iter().map(|d| &d.status).collect();
    let json = serde_json::to_value(statuses).unwrap();
    assert_eq!(json[1]["id"], "qoder");
    assert_eq!(json[1]["available"], true);
    assert_eq!(json[1]["version"], "1.1.85");
    assert_eq!(json[1]["capabilities"]["headless"], true);
    assert_eq!(json[2]["available"], false);
    assert!(json[2]["capabilities"].is_null());
    assert!(json[2]["reason"].as_str().unwrap().contains("not found"));
}
