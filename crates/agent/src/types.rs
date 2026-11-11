//! The public driver contract. Shape is PROPOSAL.md "The data shape / Job"
//! and research/05's proposed types; later tickets 07, 10, 11, 12 depend on it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Child;
use tokio::sync::{mpsc, Mutex};

fn is_false(value: &bool) -> bool {
    !*value
}

/// Effort passed to `--effort`. Any lowercase word the CLI reports, not a
/// fixed list. A leading `-` is rejected so the value cannot become a flag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Effort(String);

impl Effort {
    /// The flag value, for example `xhigh`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A level is lowercase letters. Empty, a leading `-`, and anything else
    /// are errors, not a default.
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.is_empty() {
            return Err("effort is empty".to_string());
        }
        if raw.starts_with('-') {
            return Err("effort must not start with '-'".to_string());
        }
        if !raw.chars().all(|c| c.is_ascii_lowercase()) {
            return Err("effort must be lowercase letters".to_string());
        }
        Ok(Self(raw.to_string()))
    }
}

impl TryFrom<String> for Effort {
    type Error = String;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(&raw)
    }
}

impl From<Effort> for String {
    fn from(effort: Effort) -> Self {
        effort.0
    }
}

/// A model id safe to place after `--model`.
///
/// Empty, a leading `-`, and any character outside `A-Za-z0-9._:-` are
/// rejected, so the value cannot become another flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelId(String);

impl ModelId {
    /// Reject a value that must not be passed as `--model`.
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.is_empty() {
            return Err("model is empty".to_string());
        }
        if raw.starts_with('-') {
            return Err("model must not start with '-'".to_string());
        }
        if !raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
        {
            return Err("model may contain only A-Za-z0-9._:-".to_string());
        }
        Ok(Self(raw.to_string()))
    }

    /// The id as the CLI receives it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for ModelId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ModelId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// How the CLI authenticates when no metering env var is present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AuthKind {
    /// Existing interactive subscription login (OAuth/keychain).
    SubscriptionOAuth,
    /// Provider API keys; pay-as-you-go.
    ProviderKeys,
    /// Either path works.
    Either,
}

/// What a probed driver supports. Never hardcoded: probed from the CLI.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Capabilities {
    pub headless: bool,
    pub stream_json: bool,
    pub structured_output: bool,
    pub resume: bool,
    pub system_append: bool,
    /// The CLI can limit a job to `JobSpec::allowed_tools`, not only
    /// pre-approve them.
    pub tool_allowlist: bool,
    /// The CLI has a mode that keeps a job from changing files. False means a
    /// read-only job is refused.
    pub read_only: bool,
    /// `--help` lists `--model`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub model: bool,
    /// `--help` lists `--effort`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub effort: bool,
    pub auth: AuthKind,
}

/// One agent invocation: a prompt in, structured output out, resumable.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct JobSpec {
    pub prompt: String,
    pub system_append: Option<String>,
    pub workdir: PathBuf,
    pub json_schema: Option<serde_json::Value>,
    /// Map to plan mode (no writes) versus edit-capable mode.
    pub read_only: bool,
    /// `None` runs until the child exits or the caller cancels the job.
    /// `Some` is a wall-clock limit the runner enforces by killing the child.
    #[serde(default)]
    pub timeout: Option<Duration>,
    /// The only tools the job may use, named as the CLI names them. `None`
    /// leaves the CLI's default set. A driver that cannot enforce a list
    /// refuses the job.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    /// Passed as `--model` when the probed CLI advertises the flag. Unset
    /// passes nothing, and the CLI keeps its own default.
    #[serde(default)]
    pub model: Option<ModelId>,
    /// Passed as `--effort` when the probed CLI advertises the flag. Unset
    /// passes nothing.
    #[serde(default)]
    pub effort: Option<Effort>,
}

impl JobSpec {
    /// A read-only job with no wall-clock limit. The caller cancels a stuck job.
    pub fn new(prompt: impl Into<String>, workdir: impl Into<PathBuf>) -> Self {
        Self {
            prompt: prompt.into(),
            system_append: None,
            workdir: workdir.into(),
            json_schema: None,
            read_only: true,
            timeout: None,
            allowed_tools: None,
            model: None,
            effort: None,
        }
    }
}

/// `--model` and `--effort` for a job, only when the field is set and `caps`
/// says the probed `--help` lists the flag.
pub(crate) fn push_selection(args: &mut Vec<String>, job: &JobSpec, caps: &Capabilities) {
    if let Some(model) = job.model.as_ref().filter(|_| caps.model) {
        args.push("--model".to_string());
        args.push(model.as_str().to_string());
    }
    if let Some(effort) = job.effort.as_ref().filter(|_| caps.effort) {
        args.push("--effort".to_string());
        args.push(effort.as_str().to_string());
    }
}

/// One parsed event from the child's NDJSON stdout.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AgentEvent {
    Session {
        id: String,
    },
    TextDelta(String),
    /// The assistant message's text deltas are complete. The job log flushes
    /// them into one line here.
    MessageBoundary,
    ToolUse {
        name: String,
        /// File path, search pattern, or URL when the stream gave one.
        #[serde(default)]
        detail: Option<String>,
    },
    Result {
        value: serde_json::Value,
    },
    Error(String),
    Done,
}

/// A running job. The child lives behind a shared lock so `cancel` can kill
/// it by handle; `kill_on_drop` covers the dropped-handle case.
pub struct JobHandle {
    pub pid: Option<u32>,
    /// Binary plus arguments, the same vec the driver passed to spawn.
    pub command: Vec<String>,
    pub(crate) child: Arc<Mutex<Option<Child>>>,
    pub(crate) rx: Arc<Mutex<mpsc::Receiver<AgentEvent>>>,
    pub(crate) cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl JobHandle {
    /// A handle with no child process whose stream yields `events` in order
    /// and then ends. For a driver that is not a local CLI, such as a test
    /// fake in another crate.
    pub fn scripted(events: impl IntoIterator<Item = AgentEvent>) -> Self {
        let events: Vec<AgentEvent> = events.into_iter().collect();
        let (tx, rx) = mpsc::channel(events.len().max(1));
        for event in events {
            // The channel holds every event, so a send never finds it full.
            let _ = tx.try_send(event);
        }
        Self {
            pid: None,
            command: Vec::new(),
            child: Arc::new(Mutex::new(None)),
            rx: Arc::new(Mutex::new(rx)),
            cancelled: Arc::default(),
        }
    }

    /// `scripted`, with the argv a real spawn would have used.
    pub fn scripted_with_command(
        command: Vec<String>,
        events: impl IntoIterator<Item = AgentEvent>,
    ) -> Self {
        let mut handle = Self::scripted(events);
        handle.command = command;
        handle
    }

    /// A stream that stays open until the sender is dropped. Tests cancel a
    /// job by flagging the handle and dropping the sender.
    pub fn open_stream(command: Vec<String>) -> (Self, mpsc::Sender<AgentEvent>) {
        let (tx, rx) = mpsc::channel(8);
        (
            Self {
                pid: None,
                command,
                child: Arc::new(Mutex::new(None)),
                rx: Arc::new(Mutex::new(rx)),
                cancelled: Arc::default(),
            },
            tx,
        )
    }

    /// Ask the runner to stop. The same flag `cancel` sets on a live child.
    pub fn request_cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Whether `request_cancel` has run.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Shared with the registry so a cancel command can raise the flag.
    pub fn cancel_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.cancelled)
    }
}

/// The job's event stream. Cloned streams share one receiver: each event is
/// delivered to exactly one consumer.
#[derive(Clone)]
pub struct EventStream {
    pub(crate) rx: Arc<Mutex<mpsc::Receiver<AgentEvent>>>,
}

impl EventStream {
    /// The stream of the job behind `h`. Every stream of one job shares its
    /// receiver.
    pub fn for_job(h: &JobHandle) -> Self {
        Self { rx: h.rx.clone() }
    }

    /// Next event, or `None` once the job is done and the channel is drained.
    pub async fn recv(&mut self) -> Option<AgentEvent> {
        self.rx.lock().await.recv().await
    }
}

/// The driver contract from PROPOSAL.md. Do not redesign; four later tickets
/// depend on this shape.
#[async_trait::async_trait]
pub trait AgentDriver: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    async fn start(&self, job: JobSpec) -> anyhow::Result<JobHandle>;
    async fn resume(&self, session: &str, job: JobSpec) -> anyhow::Result<JobHandle>;
    fn events(&self, h: &JobHandle) -> EventStream;
    async fn cancel(&self, h: &JobHandle) -> anyhow::Result<()>;
}
