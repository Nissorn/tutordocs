//! The hook every agent job calls so the app can keep a log.
//!
//! The registry lives in the shell. A command sets it for the task that runs
//! the job. A caller outside that scope, such as an engine test that does not
//! care about the log, changes nothing.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tutordocs_agent::{AgentEvent, Folded, LogFold};

use crate::time::rfc3339_utc;

const MAX_JOBS: usize = 50;
const MAX_LINES: usize = 2000;

pub type EmitFn = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// One job as `list_agent_jobs` and `agent_job` return it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub id: String,
    pub course_id: String,
    pub kind: JobKind,
    pub driver: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub command: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: JobStatus,
    pub lines: Vec<LogLine>,
}

/// `running`, then exactly one of `succeeded`, `failed`, `cancelled`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// One log line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub at: String,
    pub kind: LineKind,
    pub text: String,
}

/// `text`, `tool`, `error`, or `status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Text,
    Tool,
    Error,
    Status,
}

struct Live {
    cancel: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

struct Slot {
    record: JobRecord,
    fold: LogFold,
    live: Option<Live>,
    /// The last line is already the truncation marker.
    full: bool,
}

struct Inner {
    next: u64,
    slots: VecDeque<Slot>,
}

/// App-wide registry. Cheap to clone: every clone shares the same records.
#[derive(Clone)]
pub struct JobRegistry {
    inner: Arc<Mutex<Inner>>,
    emit: Arc<Mutex<Option<EmitFn>>>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                next: 1,
                slots: VecDeque::new(),
            })),
            emit: Arc::new(Mutex::new(None)),
        }
    }

    /// `emit` receives `(event name, payload)` for line and status events.
    /// The shell installs the Tauri emitter here; the CLI installs a stderr
    /// writer. Tests that never attach still keep the records.
    pub fn attach_emit(&self, emit: EmitFn) {
        *lock(&self.emit) = Some(emit);
    }

    /// The log a command installs for the task that runs its job.
    pub fn log(&self) -> Arc<dyn JobLog> {
        Arc::new(self.clone())
    }

    pub fn list(&self) -> Vec<JobRecord> {
        lock(&self.inner)
            .slots
            .iter()
            .map(|slot| slot.record.clone())
            .collect()
    }

    pub fn get(&self, id: &str) -> Result<JobRecord, String> {
        lock(&self.inner)
            .slots
            .iter()
            .find(|slot| slot.record.id == id)
            .map(|slot| slot.record.clone())
            .ok_or_else(|| format!("no job {id}"))
    }

    /// Raise the cancel flag of a running job and wake its reader.
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let mut inner = lock(&self.inner);
        let slot = inner
            .slots
            .iter_mut()
            .find(|slot| slot.record.id == id)
            .ok_or_else(|| format!("no job {id}"))?;
        if slot.record.status != JobStatus::Running {
            return Err(format!("job {id} is not running"));
        }
        let Some(live) = slot.live.as_ref() else {
            return Err(format!("job {id} is not running"));
        };
        live.cancel.store(true, Ordering::SeqCst);
        (live.wake)();
        Ok(())
    }

    fn emit(&self, name: &str, payload: Value) {
        let emit = lock(&self.emit).clone();
        if let Some(emit) = emit {
            emit(name, payload);
        }
    }

    fn accept(&self, id: &str, line: LogLine) -> Option<LogLine> {
        let mut inner = lock(&self.inner);
        let slot = inner.slots.iter_mut().find(|slot| slot.record.id == id)?;
        if slot.full {
            return None;
        }
        if slot.record.lines.len() < MAX_LINES {
            slot.record.lines.push(line.clone());
            return Some(line);
        }
        // A later line does not fit, so the last kept line says so.
        slot.full = true;
        let marker = LogLine {
            at: line.at,
            kind: LineKind::Status,
            text: "truncated".to_string(),
        };
        if let Some(last) = slot.record.lines.last_mut() {
            *last = marker.clone();
        }
        Some(marker)
    }

    fn push_folded(&self, id: &str, folded: Folded) {
        let (kind, text) = match folded {
            Folded::Text(text) => (LineKind::Text, text),
            Folded::Tool { name, detail } => {
                let text = match detail {
                    Some(detail) => format!("{name} {detail}"),
                    None => name,
                };
                (LineKind::Tool, text)
            }
            Folded::Error(text) => (LineKind::Error, text),
        };
        self.push_status_like(id, kind, text);
    }

    fn push_status_like(&self, id: &str, kind: LineKind, text: String) {
        let line = LogLine {
            at: rfc3339_utc(SystemTime::now()),
            kind,
            text,
        };
        if let Some(line) = self.accept(id, line) {
            self.emit("agent-job-line", json!({ "jobId": id, "line": line }));
        }
    }
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl JobLog for JobRegistry {
    fn begin(&self, start: JobStart) -> String {
        let (id, at) = {
            let mut inner = lock(&self.inner);
            let id = format!("job-{}", inner.next);
            inner.next += 1;
            if inner.slots.len() == MAX_JOBS {
                inner.slots.pop_front();
            }
            let at = rfc3339_utc(SystemTime::now());
            inner.slots.push_back(Slot {
                record: JobRecord {
                    id: id.clone(),
                    course_id: start.course_id,
                    kind: start.kind,
                    driver: start.driver,
                    model: start.model,
                    effort: start.effort,
                    command: start.command,
                    started_at: at.clone(),
                    ended_at: None,
                    status: JobStatus::Running,
                    lines: vec![LogLine {
                        at: at.clone(),
                        kind: LineKind::Status,
                        text: "running".to_string(),
                    }],
                },
                fold: LogFold::new(),
                live: Some(Live {
                    cancel: start.cancel,
                    wake: start.wake,
                }),
                full: false,
            });
            (id, at)
        };
        self.emit(
            "agent-job-line",
            json!({
                "jobId": &id,
                "line": { "at": at, "kind": "status", "text": "running" }
            }),
        );
        self.emit(
            "agent-job-status",
            json!({ "jobId": &id, "status": "running" }),
        );
        id
    }

    fn record(&self, id: &str, event: &AgentEvent) {
        let folded = {
            let mut inner = lock(&self.inner);
            let Some(slot) = inner.slots.iter_mut().find(|slot| slot.record.id == id) else {
                return;
            };
            slot.fold.push(event)
        };
        for line in folded {
            self.push_folded(id, line);
        }
    }

    fn finish(&self, id: &str, outcome: JobOutcome) {
        let (status, text) = match outcome {
            JobOutcome::Succeeded => (JobStatus::Succeeded, "succeeded"),
            JobOutcome::Failed => (JobStatus::Failed, "failed"),
            JobOutcome::Cancelled => (JobStatus::Cancelled, "cancelled"),
        };
        let tail = {
            let mut inner = lock(&self.inner);
            let Some(slot) = inner.slots.iter_mut().find(|slot| slot.record.id == id) else {
                return;
            };
            if slot.record.status != JobStatus::Running {
                return;
            }
            slot.live = None;
            slot.record.status = status;
            slot.record.ended_at = Some(rfc3339_utc(SystemTime::now()));
            slot.fold.finish()
        };
        if let Some(tail) = tail {
            self.push_folded(id, tail);
        }
        self.push_status_like(id, LineKind::Status, text.to_string());
        self.emit("agent-job-status", json!({ "jobId": id, "status": text }));
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

tokio::task_local! {
    static ACTIVE_LOG: Arc<dyn JobLog>;
}

/// Which command started the job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    Outline,
    Chapter,
    Rewrite,
    Url,
    Quiz,
    Grade,
    Ask,
}

/// How a running job ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

/// What the registry records when a process has been spawned, or when spawn
/// itself failed. `command` is already redacted.
pub struct JobStart {
    pub course_id: String,
    pub kind: JobKind,
    pub driver: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub command: String,
    pub cancel: Arc<AtomicBool>,
    /// Wakes the job's reader after `cancel` is set. A permit is kept if the
    /// reader has not reached its wait yet.
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

/// App-wide job log. The shell implements it.
pub trait JobLog: Send + Sync {
    fn begin(&self, start: JobStart) -> String;
    fn record(&self, id: &str, event: &AgentEvent);
    fn finish(&self, id: &str, outcome: JobOutcome);
}

/// Run `fut` with `log` visible to [`with_log`].
pub async fn job_scope<T>(log: Arc<dyn JobLog>, fut: impl Future<Output = T>) -> T {
    ACTIVE_LOG.scope(log, fut).await
}

pub(crate) fn with_log<T>(f: impl FnOnce(&dyn JobLog) -> T) -> Option<T> {
    ACTIVE_LOG.try_with(|log| f(log.as_ref())).ok()
}

/// The value after `--model` or `--effort` in the spawned argv.
pub(crate) fn flag_value(argv: &[String], flag: &str) -> Option<String> {
    argv.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
}

/// Join `argv` for the record, with the prompt and system text replaced by a
/// length placeholder. An argument that merely contains either string is
/// replaced whole, because a driver that cannot pass system text inlines it
/// into the prompt argument.
pub(crate) fn redact_command(argv: &[String], prompt: &str, system: Option<&str>) -> String {
    argv.iter()
        .map(|arg| redact_arg(arg, prompt, system))
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_arg(arg: &str, prompt: &str, system: Option<&str>) -> String {
    if !prompt.is_empty() && (arg == prompt || arg.contains(prompt)) {
        return format!("<prompt, {} chars>", prompt.chars().count());
    }
    if let Some(system) = system.filter(|text| !text.is_empty()) {
        if arg == system || arg.contains(system) {
            return format!("<system, {} chars>", system.chars().count());
        }
    }
    arg.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_replaces_the_prompt_and_the_system_text_with_their_lengths() {
        let argv = vec![
            "claude".into(),
            "-p".into(),
            "hello there".into(),
            "--append-system-prompt".into(),
            "be brief".into(),
            "--model".into(),
            "sonnet".into(),
        ];
        assert_eq!(
            redact_command(&argv, "hello there", Some("be brief")),
            "claude -p <prompt, 11 chars> --append-system-prompt <system, 8 chars> --model sonnet"
        );
    }

    #[test]
    fn redact_replaces_a_prompt_argument_that_also_holds_the_system_text() {
        let prompt = "hello there";
        let system = "be brief";
        let shaped = format!("Instructions:\n{system}\n\nTask:\n{prompt}");
        let argv = vec!["agy".into(), "-p".into(), shaped];
        let command = redact_command(&argv, prompt, Some(system));
        assert_eq!(command, "agy -p <prompt, 11 chars>");
        assert!(!command.contains(prompt));
        assert!(!command.contains(system));
    }

    #[test]
    fn flag_value_reads_the_spawned_model_and_effort() {
        let argv = ["agy", "--model", "gemini-3", "--effort", "low"]
            .map(str::to_string)
            .to_vec();
        assert_eq!(flag_value(&argv, "--model").as_deref(), Some("gemini-3"));
        assert_eq!(flag_value(&argv, "--effort").as_deref(), Some("low"));
        assert_eq!(flag_value(&argv, "--resume"), None);
    }
}
