//! The keyless Claude Code driver: spawns `claude -p --output-format
//! stream-json` with an explicitly scrubbed environment, parses NDJSON into
//! `AgentEvent`s, validates structured results, and kills by handle.
//!
//! Flag surface is probed from `--version`/`--help` at construction because
//! the CLI auto-updates (PROPOSAL.md "Agent drivers"). The env scrub is a
//! TutorDocs addition; see `crate::env`.

use std::path::PathBuf;

use anyhow::bail;

use crate::parse::NdjsonParser;
use crate::policy::{allowlist_edits_files, require_enforceable, tool_args};
use crate::probe::{advertises_option, run_probe};
use crate::runner::{self, Launch};
use crate::types::{AgentDriver, AuthKind, Capabilities, EventStream, JobHandle, JobSpec};

/// Drives the locally installed, subscription-logged-in Claude Code CLI.
pub struct ClaudeDriver {
    binary: PathBuf,
    version: String,
    caps: Capabilities,
    tap: Option<PathBuf>,
}

impl ClaudeDriver {
    /// Probe the CLI once. Fails if the binary is missing or `--version`
    /// fails; capabilities degrade from what `--help` actually advertises.
    pub fn new(binary: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let binary = binary.into();
        let probe = run_probe(&binary)?;
        let caps = Self::capabilities_from_help(&probe.help);
        Ok(Self {
            binary,
            version: probe.version,
            caps,
            tap: None,
        })
    }

    /// The pure probe: what a `claude --help` text advertises.
    pub fn capabilities_from_help(help_text: &str) -> Capabilities {
        Capabilities {
            headless: help_text.contains("--print"),
            stream_json: help_text.contains("stream-json"),
            structured_output: help_text.contains("--json-schema"),
            resume: help_text.contains("--resume"),
            system_append: help_text.contains("--append-system-prompt"),
            tool_allowlist: advertises_option(help_text, "--tools")
                && (advertises_option(help_text, "--allowedTools")
                    || advertises_option(help_text, "--allowed-tools")),
            // Read-only jobs pass `--permission-mode plan`. A CLI that lost
            // the option or plan mode rejects the argv and the job fails
            // loudly, so a mention of the option is enough.
            read_only: help_text.contains("--permission-mode"),
            model: advertises_option(help_text, "--model"),
            effort: advertises_option(help_text, "--effort"),
            auth: AuthKind::SubscriptionOAuth,
        }
    }

    /// What `--version` printed at construction.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Record every raw stdout line of each job to `path` (truncated per job),
    /// for fixtures and spike evidence. Opt-in per driver, never read from env.
    pub fn with_ndjson_tap(mut self, path: impl Into<PathBuf>) -> Self {
        self.tap = Some(path.into());
        self
    }

    fn check_degradation(&self, job: &JobSpec, resume: Option<&str>) -> anyhow::Result<()> {
        if !self.caps.headless || !self.caps.stream_json {
            bail!("probed claude CLI lacks print/stream-json support; cannot drive headlessly");
        }
        if job.json_schema.is_some() && !self.caps.structured_output {
            bail!("probed claude CLI lacks --json-schema; structured output unavailable");
        }
        if job.system_append.is_some() && !self.caps.system_append {
            bail!("probed claude CLI lacks --append-system-prompt");
        }
        if resume.is_some() && !self.caps.resume {
            bail!("probed claude CLI lacks --resume");
        }
        require_enforceable("claude", job, &self.caps)
    }

    async fn spawn(&self, job: JobSpec, resume: Option<&str>) -> anyhow::Result<JobHandle> {
        self.check_degradation(&job, resume)?;
        runner::launch(
            Launch {
                label: "claude",
                binary: &self.binary,
                args: build_args(&job, resume, &self.caps),
                workdir: &job.workdir,
                extra_scrub: &[],
                tap: self.tap.as_deref(),
                timeout: job.timeout,
                schema: job.json_schema.as_ref(),
                cleanup_dir: None,
            },
            NdjsonParser::new(),
        )
        .await
    }
}

/// argv for one job, a pure function of the job and the probed capabilities.
/// The driver refuses a job the probed CLI cannot run before building it.
pub fn build_args(job: &JobSpec, resume: Option<&str>, caps: &Capabilities) -> Vec<String> {
    // A file-editing tool under `--dangerously-skip-permissions` selects
    // bypassPermissions, which approves a write anywhere. The skip flag stays
    // only when the allowlist names none of the file-editing tools in
    // `policy`. A writing allowlist uses acceptEdits, which approves edits
    // inside the working directory. Hooks and foreign MCP servers stay off
    // either way. The CLI rejects `--print --output-format stream-json`
    // without `--verbose` ("requires --verbose", read from claude 2.1.319).
    let edits = allowlist_edits_files(job);
    let mut args = vec![
        "-p".to_string(),
        job.prompt.clone(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
    ];
    if !edits {
        args.push("--dangerously-skip-permissions".to_string());
    }
    args.extend([
        // No `--mcp-config`: strict mode then loads none of the user's servers.
        "--strict-mcp-config".to_string(),
        "--settings".to_string(),
        "{\"disableAllHooks\":true}".to_string(),
        // Hook lifecycle lines are omitted unless this is set. The proof job
        // needs them in the stream, so a hook that still runs is visible.
        "--include-hook-events".to_string(),
        "--permission-mode".to_string(),
        if (edits && job.allowed_tools.is_some()) || !job.read_only {
            "acceptEdits"
        } else {
            "plan"
        }
        .to_string(),
    ]);
    if let Some(schema) = &job.json_schema {
        args.push("--json-schema".to_string());
        args.push(schema.to_string());
    }
    if let Some(append) = &job.system_append {
        args.push("--append-system-prompt".to_string());
        args.push(append.clone());
    }
    args.extend(tool_args(job, "--tools", "--allowedTools"));
    if let Some(session) = resume {
        args.push("--resume".to_string());
        args.push(session.to_string());
    }
    crate::types::push_selection(&mut args, job, caps);
    args
}

#[async_trait::async_trait]
impl AgentDriver for ClaudeDriver {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn capabilities(&self) -> Capabilities {
        self.caps.clone()
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
        runner::kill(h, "claude").await
    }
}
