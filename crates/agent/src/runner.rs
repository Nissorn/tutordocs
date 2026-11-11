//! The provider-neutral half of every driver: spawn the child with a scrubbed
//! environment, pump its NDJSON stdout through a provider parser, honor an
//! optional job limit, validate structured results and reap the child.
//!
//! A job with no limit runs until the child exits or the caller cancels it.
//! Cancellation comes from the job log. A `Some` limit is the pump's own
//! kill, used by the examples and by tests. agy has a reported non-TTY hang
//! (research/05), and a hung child never closes stdout, so with no limit the
//! pump stays open until that cancel.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, Mutex};

use crate::env::child_env_with;
use crate::types::{AgentEvent, JobHandle};

/// Turns one stdout line into events. One instance per job.
pub(crate) trait LineParser: Send + 'static {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent>;
}

/// Everything `launch` needs about one job.
pub(crate) struct Launch<'a> {
    /// Driver name used in error text, such as `claude`.
    pub label: &'static str,
    pub binary: &'a Path,
    pub args: Vec<String>,
    pub workdir: &'a Path,
    /// The provider's own metering variables, scrubbed on top of Claude's.
    pub extra_scrub: &'static [&'static str],
    pub tap: Option<&'a Path>,
    pub timeout: Option<Duration>,
    pub schema: Option<&'a serde_json::Value>,
    /// A directory to remove when the child exits, or when launch fails.
    /// The agy driver puts its sandbox profile here, outside the job folder.
    pub cleanup_dir: Option<PathBuf>,
}

/// Spawn the child and start pumping. Fails before spawning on an invalid
/// schema or an unwritable tap, so a bad job never leaves a child running.
pub(crate) async fn launch(l: Launch<'_>, parser: impl LineParser) -> anyhow::Result<JobHandle> {
    // Owns the profile directory from here on, including every error return
    // before the pump task takes it.
    let cleanup = DirGuard(l.cleanup_dir);
    let validator = match l.schema {
        Some(schema) => Some(
            jsonschema::validator_for(schema)
                .map_err(|e| anyhow::anyhow!("job schema is invalid: {e}"))?,
        ),
        None => None,
    };
    let tap = match l.tap {
        Some(path) => Some(
            std::fs::File::create(path)
                .with_context(|| format!("cannot create NDJSON tap {}", path.display()))?,
        ),
        None => None,
    };

    let mut cmd = tokio::process::Command::new(l.binary);
    cmd.args(&l.args)
        .current_dir(l.workdir)
        // Explicit child environment: the scrub is the keyless guarantee.
        .env_clear()
        .envs(child_env_with(l.extra_scrub))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawning {} failed", l.label))?;
    let pid = child.id();
    let stdout = child.stdout.take().context("child stdout not piped")?;
    let stderr = child.stderr.take().context("child stderr not piped")?;

    let (tx, rx) = mpsc::channel::<AgentEvent>(64);
    let child = Arc::new(Mutex::new(Some(child)));
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pump_child = child.clone();
    let pump_cancelled = cancelled.clone();
    let policy = PumpPolicy {
        label: l.label,
        validator,
        timeout: l.timeout,
        tap,
        cleanup,
    };
    tokio::spawn(async move {
        pump(
            stdout,
            stderr,
            tx,
            pump_child,
            pump_cancelled,
            policy,
            parser,
        )
        .await;
    });

    // The job log prints this vec. Rebuilding the flags later would drift from the spawn.
    let mut command = Vec::with_capacity(1 + l.args.len());
    command.push(l.binary.to_string_lossy().into_owned());
    command.extend(l.args.iter().cloned());
    Ok(JobHandle {
        pid,
        command,
        child,
        rx: Arc::new(Mutex::new(rx)),
        cancelled,
    })
}

/// Kill the job's child by handle. The pump reaps; `kill_on_drop` is the
/// backstop for a dropped handle.
pub(crate) async fn kill(h: &JobHandle, label: &str) -> anyhow::Result<()> {
    h.cancelled.store(true, Ordering::SeqCst);
    let mut guard = h.child.lock().await;
    if let Some(child) = guard.as_mut() {
        child
            .start_kill()
            .with_context(|| format!("killing {label} child failed"))?;
    }
    Ok(())
}

/// Per-job settings the pump needs beyond the child and its channels.
struct PumpPolicy {
    label: &'static str,
    validator: Option<jsonschema::Validator>,
    timeout: Option<Duration>,
    tap: Option<std::fs::File>,
    cleanup: DirGuard,
}

/// Removes a directory when dropped. Used for the agy sandbox profile, which
/// must outlive the child and must not remain after it.
struct DirGuard(Option<PathBuf>);

impl Drop for DirGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

/// Reads the child's NDJSON stdout until EOF, cancellation, or a `Some` limit.
async fn pump(
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
    tx: mpsc::Sender<AgentEvent>,
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    policy: PumpPolicy,
    mut parser: impl LineParser,
) {
    let PumpPolicy {
        label,
        validator,
        timeout,
        mut tap,
        cleanup,
    } = policy;
    // Drain stderr concurrently so a chatty child cannot block on a full pipe.
    let stderr_task = tokio::spawn(async move {
        let mut buf = String::new();
        let mut stderr = stderr;
        let _ = stderr.read_to_string(&mut buf).await;
        buf
    });

    let mut lines = BufReader::new(stdout).lines();
    // `None` constructs no Sleep, so the select arm below stays disabled.
    let limited = timeout.is_some();
    let deadline = timeout.map(tokio::time::sleep);
    tokio::pin!(deadline);
    let mut timed_out = false;
    let mut concluded = false;
    let mut diagnostics = Diagnostics::default();

    loop {
        tokio::select! {
            res = lines.next_line() => match res {
                Ok(Some(line)) => {
                    if let Some(f) = tap.as_mut() {
                        use std::io::Write;
                        let _ = writeln!(f, "{line}");
                    }
                    diagnostics.note(&line);
                    for ev in parser.parse_line(&line) {
                        let ev = match ev {
                            AgentEvent::Result { value } => match &validator {
                                Some(v) => match v.validate(&value) {
                                    Ok(()) => AgentEvent::Result { value },
                                    Err(e) => AgentEvent::Error(format!(
                                        "result failed schema validation: {e}"
                                    )),
                                },
                                None => AgentEvent::Result { value },
                            },
                            other => other,
                        };
                        concluded |= matches!(ev, AgentEvent::Result { .. } | AgentEvent::Error(_));
                        if tx.send(ev).await.is_err() {
                            return; // consumer gone; kill_on_drop covers the child
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = tx.send(AgentEvent::Error(format!("reading {label} stdout: {e}"))).await;
                    break;
                }
            },
            _ = async {
                match deadline.as_mut().as_pin_mut() {
                    Some(sleep) => sleep.await,
                    None => std::future::pending().await,
                }
            }, if limited => {
                timed_out = true;
                let _ = tx.send(AgentEvent::Error("job timed out; child killed".to_string())).await;
                if let Some(c) = child.lock().await.as_mut() {
                    let _ = c.start_kill();
                }
                break;
            }
        }
    }

    // Reap the child so no zombie remains, then report abnormal exit.
    let status = {
        let mut guard = child.lock().await;
        match guard.as_mut() {
            Some(c) => c.wait().await.ok(),
            None => None,
        }
    };
    let was_cancelled = cancelled.load(Ordering::SeqCst);
    if !timed_out && !was_cancelled {
        let failure = match status {
            Some(status) if !status.success() => {
                let stderr_text = stderr_task.await.unwrap_or_default();
                Some(auth_failure(label, &stderr_text).unwrap_or_else(|| {
                    format!("{label} exited with {status}: {}", clip(stderr_text.trim()))
                }))
            }
            _ if !concluded => Some(format!(
                "{label} ended without a result event; last output: {}",
                diagnostics.summary()
            )),
            _ => None,
        };
        if let Some(message) = failure {
            let _ = tx.send(AgentEvent::Error(message)).await;
        }
    }
    // The child has been reaped. Drop the profile directory before Done, so a
    // waiter that has observed Done cannot still see the file.
    drop(cleanup);
    let _ = tx.send(AgentEvent::Done).await;
}

fn clip(text: &str) -> String {
    text.chars().take(500).collect()
}

/// Child stderr that means the login is missing or expired, matched without
/// regard to case.
const AUTH_MARKERS: &[&str] = &[
    "authentication required",
    "not logged in",
    "please visit the url to log in",
    "authentication failed",
];

/// What the user runs to sign one agent in again. A label with no row gets the
/// generic remedy.
const AUTH_REMEDIES: &[(&str, &str)] = &[
    (
        "agy",
        "run `agy` once in Terminal and sign in with the Google account",
    ),
    ("claude", "run `claude` once in Terminal and use /login"),
    ("qoder", "run `qoder` once in Terminal and sign in"),
];

/// A login failure names the agent and the remedy. Any other failure returns
/// `None`, because there the raw stderr tells the user more than a guess would.
fn auth_failure(label: &str, stderr: &str) -> Option<String> {
    let lower = stderr.to_lowercase();
    if !AUTH_MARKERS.iter().any(|marker| lower.contains(marker)) {
        return None;
    }
    let remedy = AUTH_REMEDIES
        .iter()
        .find(|(name, _)| *name == label)
        .map(|(_, remedy)| *remedy)
        .unwrap_or("sign in again in Terminal");
    Some(format!("{label} needs a sign-in: {remedy}"))
}

/// The child's most recent non-JSON stdout lines. They are never job errors on
/// their own; they explain a job that ended without a result.
#[derive(Default)]
struct Diagnostics(std::collections::VecDeque<String>);

impl Diagnostics {
    const KEEP: usize = 5;

    fn note(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() || serde_json::from_str::<serde::de::IgnoredAny>(line).is_ok() {
            return;
        }
        if self.0.len() == Self::KEEP {
            self.0.pop_front();
        }
        self.0.push_back(line.to_string());
    }

    fn summary(&self) -> String {
        if self.0.is_empty() {
            return "(none)".to_string();
        }
        clip(&Vec::from(self.0.clone()).join(" | "))
    }
}

#[cfg(test)]
mod tests {
    use super::auth_failure;

    const AGY_STDERR: &str = "Authentication required. Please visit the URL to log in";

    #[test]
    fn the_real_agy_login_prompt_names_agy_and_the_google_sign_in() {
        assert_eq!(
            auth_failure("agy", AGY_STDERR).as_deref(),
            Some(
                "agy needs a sign-in: run `agy` once in Terminal and sign in with the Google account"
            )
        );
    }

    #[test]
    fn a_claude_login_failure_names_the_login_command() {
        assert_eq!(
            auth_failure("claude", "Not logged in. Please run /login.").as_deref(),
            Some("claude needs a sign-in: run `claude` once in Terminal and use /login")
        );
    }

    #[test]
    fn a_qoder_login_failure_names_qoder() {
        assert_eq!(
            auth_failure("qoder", "authentication failed").as_deref(),
            Some("qoder needs a sign-in: run `qoder` once in Terminal and sign in")
        );
    }

    #[test]
    fn an_unknown_agent_gets_the_generic_remedy() {
        assert_eq!(
            auth_failure("other", AGY_STDERR).as_deref(),
            Some("other needs a sign-in: sign in again in Terminal")
        );
    }

    #[test]
    fn every_marker_matches_without_regard_to_case() {
        for stderr in [
            "AUTHENTICATION REQUIRED",
            "Not Logged In",
            "Please visit the URL to log in",
            "Authentication Failed",
        ] {
            assert!(auth_failure("agy", stderr).is_some(), "{stderr}");
        }
    }

    #[test]
    fn a_failure_that_is_not_about_login_keeps_the_raw_stderr() {
        assert_eq!(auth_failure("agy", "cannot reach the model endpoint"), None);
    }
}
