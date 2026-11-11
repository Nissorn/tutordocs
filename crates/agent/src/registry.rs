//! Which agent CLIs are installed on this machine, for the driver picker.
//!
//! A GUI app launched from Finder inherits a minimal PATH that lacks
//! `~/.local/bin`, where all three CLIs live here, so lookup walks PATH and
//! then the known install dirs. Every driver is always listed: an absent or
//! unusable CLI is reported with a reason, never assumed.

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agy::AgyDriver;
use crate::claude::ClaudeDriver;
use crate::qoder::QoderDriver;
use crate::types::{AgentDriver, Capabilities};

/// What the UI shows for one driver. Serializes as the Tauri payload.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DriverStatus {
    pub id: String,
    pub available: bool,
    pub binary: Option<PathBuf>,
    pub version: Option<String>,
    pub capabilities: Option<Capabilities>,
    /// Why the driver is not available: not found, or found but unusable.
    pub reason: Option<String>,
}

/// A status plus, when available, the probed driver. The driver is the cached
/// instance: capabilities were read once at detection.
#[derive(Clone)]
pub struct Detected {
    pub status: DriverStatus,
    pub driver: Option<Arc<dyn AgentDriver>>,
}

/// Driver id and the executable names to look for, preferred first. Qoder's
/// `qodercli` is the CLI itself; `qoder` is a dispatcher that can route to the
/// IDE, so it is the fallback.
const CLIS: &[(&str, &[&str])] = &[
    ("claude", &["claude"]),
    ("qoder", &["qodercli", "qoder"]),
    ("agy", &["agy"]),
];

fn known_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local/bin"),
        home.join(".qoder/entry"),
        home.join(".qoder/bin/qodercli"),
        home.join(".claude/local"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// First executable named in `names`, searching PATH dirs, then known dirs.
fn locate(names: &[&str], home: &Path, path_var: Option<&OsStr>) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = path_var
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();
    dirs.extend(known_dirs(home));
    dirs.iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| is_executable(candidate))
}

fn open(id: &str, binary: &Path) -> anyhow::Result<(Arc<dyn AgentDriver>, String)> {
    Ok(match id {
        "claude" => {
            let d = ClaudeDriver::new(binary)?;
            let v = d.version().to_string();
            (Arc::new(d), v)
        }
        "qoder" => {
            let d = QoderDriver::new(binary)?;
            let v = d.version().to_string();
            (Arc::new(d), v)
        }
        _ => {
            let d = AgyDriver::new(binary)?;
            let v = d.version().to_string();
            (Arc::new(d), v)
        }
    })
}

/// Probe all three drivers under `home` and `path_var`. Always returns claude,
/// qoder and agy in that order.
pub fn detect(home: &Path, path_var: Option<&OsStr>) -> Vec<Detected> {
    CLIS.iter()
        .map(|(id, names)| {
            let unavailable = |binary: Option<PathBuf>, reason: String| Detected {
                status: DriverStatus {
                    id: (*id).to_string(),
                    available: false,
                    binary,
                    version: None,
                    capabilities: None,
                    reason: Some(reason),
                },
                driver: None,
            };
            let Some(binary) = locate(names, home, path_var) else {
                return unavailable(
                    None,
                    format!("{id} CLI not found on PATH or in the known install dirs"),
                );
            };
            match open(id, &binary) {
                Ok((driver, version)) => Detected {
                    status: DriverStatus {
                        id: (*id).to_string(),
                        available: true,
                        binary: Some(binary),
                        version: Some(version),
                        capabilities: Some(driver.capabilities()),
                        reason: None,
                    },
                    driver: Some(driver),
                },
                Err(e) => unavailable(Some(binary), format!("{id} CLI found but unusable: {e:#}")),
            }
        })
        .collect()
}

/// `detect` against this process's `HOME` and `PATH`.
pub fn detect_installed() -> Vec<Detected> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    detect(&home, std::env::var_os("PATH").as_deref())
}
