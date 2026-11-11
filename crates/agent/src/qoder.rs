//! The keyless Qoder CLI driver: spawns `qoder -p --output-format stream-json`
//! with the Qoder and Claude metering variables scrubbed.
//!
//! Qoder's stream-json protocol is Claude-Code-shaped (the qodercli 1.1.85
//! binary embeds the same `system/init`, `assistant`, `stream_event` and
//! `result` message schemas), so `NdjsonParser` is reused. Qoder's `--help`
//! lists no `--json-schema`; a job's schema is then enforced through the
//! prompt and validated after parse by the runner.

use serde_json::Value;

use crate::parse::{json_or_text, NdjsonParser};
use std::path::PathBuf;

use crate::cli::Cli;
use crate::env::QODER_METERING_ENV_VARS;
use crate::policy::tool_args;
use crate::probe::advertises_option;
use crate::prompt::shaped_prompt;
use crate::runner::{self, LineParser};
use crate::types::{
    AgentDriver, AgentEvent, AuthKind, Capabilities, EventStream, JobHandle, JobSpec,
};

/// Drives the locally installed, browser-logged-in Qoder CLI.
pub struct QoderDriver {
    cli: Cli,
}

impl QoderDriver {
    /// The pure probe: what a `qoder --help` text advertises. `--help` does not
    /// enumerate `--output-format` values, so the flag's presence stands in for
    /// stream-json; the Qoder docs list `text`, `json` and `stream-json`.
    pub fn capabilities_from_help(help_text: &str) -> Capabilities {
        Capabilities {
            headless: help_text.contains("--print"),
            stream_json: help_text.contains("--output-format"),
            structured_output: help_text.contains("--json-schema"),
            resume: help_text.contains("--resume"),
            system_append: help_text.contains("--append-system-prompt"),
            tool_allowlist: advertises_option(help_text, "--tools")
                && advertises_option(help_text, "--allowed-tools"),
            // Qoder has no plan mode. `dont_ask` denies every call that would
            // prompt, which is how Claude Code defines `dontAsk`; inferred
            // from the shared schema, not measured on a writing job.
            read_only: help_text.contains("dont_ask"),
            model: advertises_option(help_text, "--model"),
            // Qoder lists `--reasoning-effort`, not `--effort`. Only an exact
            // `--effort` option counts, so the reasoning flag does not.
            effort: advertises_option(help_text, "--effort"),
            auth: AuthKind::SubscriptionOAuth,
        }
    }
}

impl QoderDriver {
    /// Probe the CLI once. Fails if the binary is missing, hangs or fails
    /// `--version`; capabilities degrade from what `--help` advertises.
    pub fn new(binary: impl Into<PathBuf>) -> anyhow::Result<Self> {
        Ok(Self {
            cli: Cli::probe(binary.into(), Self::capabilities_from_help)?,
        })
    }

    /// What `--version` printed at construction.
    pub fn version(&self) -> &str {
        &self.cli.version
    }

    /// Record every raw stdout line of each job to `path` (truncated per job),
    /// for fixtures and spike evidence. Opt-in per driver, never read from env.
    pub fn with_ndjson_tap(mut self, path: impl Into<PathBuf>) -> Self {
        self.cli.tap = Some(path.into());
        self
    }

    async fn spawn(&self, job: JobSpec, resume: Option<&str>) -> anyhow::Result<JobHandle> {
        self.cli.require("qoder", &job, resume, "--resume")?;
        let args = build_args(&job, resume, &self.cli.caps);
        self.cli
            .launch(
                "qoder",
                QODER_METERING_ENV_VARS,
                &job,
                args,
                QoderParser::new(),
            )
            .await
    }
}

#[async_trait::async_trait]
impl AgentDriver for QoderDriver {
    fn id(&self) -> &'static str {
        "qoder"
    }

    fn capabilities(&self) -> Capabilities {
        self.cli.caps.clone()
    }

    async fn start(&self, job: JobSpec) -> anyhow::Result<JobHandle> {
        self.spawn(job, None).await
    }

    async fn resume(&self, session: &str, job: JobSpec) -> anyhow::Result<JobHandle> {
        self.spawn(job, Some(session)).await
    }

    fn events(&self, h: &JobHandle) -> EventStream {
        EventStream::for_job(h)
    }

    async fn cancel(&self, h: &JobHandle) -> anyhow::Result<()> {
        runner::kill(h, "qoder").await
    }
}

/// argv for one job, a pure function of the job and the probed capabilities.
///
/// It starts with `-p` because the `qoder` entry script routes a first
/// argument that is an existing path to the IDE. The prompt follows `--` so a
/// prompt that looks like a flag stays a prompt.
pub fn build_args(job: &JobSpec, resume: Option<&str>, caps: &Capabilities) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--permission-mode".to_string(),
        if job.read_only {
            "dont_ask"
        } else {
            "accept_edits"
        }
        .to_string(),
    ];
    if let Some(schema) = job.json_schema.as_ref().filter(|_| caps.structured_output) {
        args.push("--json-schema".to_string());
        args.push(schema.to_string());
    }
    if let Some(system) = job.system_append.as_ref().filter(|_| caps.system_append) {
        args.push("--append-system-prompt".to_string());
        args.push(system.clone());
    }
    if let Some(session) = resume {
        args.push("--resume".to_string());
        args.push(session.to_string());
    }
    args.extend(tool_args(job, "--tools", "--allowed-tools"));
    args.push("--".to_string());
    args.push(shaped_prompt(job, caps));
    args
}

/// Qoder's stream-json parser: the Claude-shaped `NdjsonParser`, plus
/// unwrapping of a JSON reply that the model put in a Markdown fence.
#[derive(Default)]
pub struct QoderParser {
    inner: NdjsonParser,
}

impl QoderParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse one stdout line into zero or more events.
    pub fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        self.inner
            .parse_line(line)
            .into_iter()
            .map(|ev| match ev {
                AgentEvent::Result {
                    value: Value::String(text),
                } => AgentEvent::Result {
                    value: json_or_text(&text),
                },
                other => other,
            })
            .collect()
    }
}

impl LineParser for QoderParser {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        QoderParser::parse_line(self, line)
    }
}
