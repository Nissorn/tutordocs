//! Capability probing: run a CLI's `--version` and `--help` once, bounded in
//! time, and hand the text to a pure per-driver function that reads flags.
//!
//! Help text is read from stdout and stderr together because agy prints its
//! `--help` on stderr (recorded in `tests/fixtures/agy/help.stderr.txt`). A
//! probe that read only stdout would see no flags and disable every capability.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context};

/// How long startup waits on one probe command before giving up.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether help text lists `flag` as an option name, at the start of a line,
/// rather than mentioning it in another option's description.
pub(crate) fn advertises_option(help_text: &str, flag: &str) -> bool {
    help_text.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with('-')
            && line
                .split([' ', ','])
                .take_while(|token| token.is_empty() || token.starts_with('-'))
                .any(|token| token == flag)
    })
}

/// What `--version` and `--help` printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    pub version: String,
    pub help: String,
}

/// Probe `binary` with the default bound.
pub fn run_probe(binary: &Path) -> anyhow::Result<Probe> {
    run_probe_within(binary, PROBE_TIMEOUT)
}

/// Probe `binary`, killing any probe command that outlives `limit`. Fails if
/// the binary cannot run or `--version` does not exit cleanly; a non-zero
/// `--help` is tolerated because only its text matters.
pub fn run_probe_within(binary: &Path, limit: Duration) -> anyhow::Result<Probe> {
    let version = run_bounded(binary, "--version", limit)?;
    if !version.success {
        bail!(
            "{} --version exited unsuccessfully: {}",
            binary.display(),
            version.stderr.trim()
        );
    }
    let help = run_bounded(binary, "--help", limit)?;
    let version_text = if version.stdout.trim().is_empty() {
        version.stderr.trim().to_string()
    } else {
        version.stdout.trim().to_string()
    };
    Ok(Probe {
        version: version_text,
        help: format!("{}\n{}", help.stdout, help.stderr),
    })
}

struct Captured {
    success: bool,
    stdout: String,
    stderr: String,
}

fn run_bounded(binary: &Path, flag: &str, limit: Duration) -> anyhow::Result<Captured> {
    let mut child = Command::new(binary)
        .arg(flag)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("cannot run {} {flag}", binary.display()))?;
    // Drain both pipes on threads so a chatty child cannot block on a full pipe.
    let mut out = child.stdout.take().context("probe stdout not piped")?;
    let mut err = child.stderr.take().context("probe stderr not piped")?;
    let out_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = out.read_to_string(&mut buf);
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = err.read_to_string(&mut buf);
        buf
    });

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{} {flag} timed out after {limit:?}", binary.display());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Ok(Captured {
        success: status.success(),
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
    })
}
