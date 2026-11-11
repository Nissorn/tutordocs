//! The keyless agy (Antigravity CLI) driver: spawns
//! `sandbox-exec -f <profile> agy -p <prompt> --output-format stream-json`
//! with the Google, Gemini and agy gateway metering variables scrubbed.
//!
//! agy's own `--sandbox` flag confines nothing under `--dangerously-skip-permissions`.
//! The profile in `crates/agent/src/agy.sb` is the guard: the job folder is
//! writable, the rest of the home directory is not, and exec is limited to the
//! agy binary and `/usr/bin/security`, which agy 1.5.5 runs to reach its keychain
//! login. The profile file lives outside the job folder and is removed when the
//! child exits.
//!
//! agy has no system-prompt flag, so system text goes through the prompt body.
//! Its `--help` goes to stderr, which the shared probe reads. A non-TTY hang
//! has been reported (research/05). The driver does not pass `--print-timeout`,
//! because a hung agy would not honor it. A job the app starts has no
//! wall-clock limit. The user stops a stuck job from the job log, and the
//! runner kills that child by handle. A caller that sets `JobSpec::timeout`
//! still gets the pump's own kill.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context};
use serde_json::Value;

use crate::parse::json_or_text;

use crate::cli::Cli;
use crate::env::AGY_METERING_ENV_VARS;
use crate::probe::advertises_option;
use crate::prompt::shaped_prompt;
use crate::runner::{self, LineParser};
use crate::types::{
    AgentDriver, AgentEvent, AuthKind, Capabilities, Effort, EventStream, JobHandle, JobSpec,
    ModelId,
};

/// The macOS helper that applies the per-job profile. A missing file refuses
/// every agy job, so a course job cannot run unguarded.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// The measured profile. `@HOME@` and `@JOB@` are filled per job.
/// `@HOME@/.local/bin/agy` is filled with the resolved agy binary, not with
/// `home` joined to `.local/bin/agy`.
const PROFILE_TEMPLATE: &str = include_str!("agy.sb");

const AGY_EXEC_PLACEHOLDER: &str = "@HOME@/.local/bin/agy";

/// Drives the locally installed, Google-account-logged-in agy CLI.
pub struct AgyDriver {
    cli: Cli,
    /// Capabilities from `--help`, before the sandbox adjusts them.
    probed: Capabilities,
    sandbox_exec: PathBuf,
}

impl AgyDriver {
    /// The pure probe: what an `agy --help` text advertises.
    pub fn capabilities_from_help(help_text: &str) -> Capabilities {
        Capabilities {
            headless: help_text.contains("--print"),
            stream_json: help_text.contains("stream-json"),
            structured_output: help_text.contains("--json-schema"),
            resume: help_text.contains("--conversation"),
            system_append: help_text.contains("--append-system-prompt"),
            // Help advertises neither. `AgyDriver` sets both when `sandbox-exec`
            // is present: the profile confines the process, which is what a
            // read-only allowlisted course job needs.
            tool_allowlist: false,
            read_only: false,
            model: advertises_option(help_text, "--model"),
            effort: advertises_option(help_text, "--effort"),
            auth: AuthKind::SubscriptionOAuth,
        }
    }
}

impl AgyDriver {
    /// Probe the CLI once. Fails if the binary is missing, hangs or fails
    /// `--version`; capabilities degrade from what `--help` advertises.
    pub fn new(binary: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let cli = Cli::probe(binary.into(), Self::capabilities_from_help)?;
        let mut driver = Self {
            probed: cli.caps.clone(),
            cli,
            sandbox_exec: PathBuf::from(SANDBOX_EXEC),
        };
        driver.apply_sandbox_caps();
        Ok(driver)
    }

    /// Use `path` as `sandbox-exec`. Tests pass a passthrough. A path that is
    /// not a file makes every job refuse and clears the sandbox capabilities.
    pub fn with_sandbox_exec(mut self, path: impl Into<PathBuf>) -> Self {
        self.sandbox_exec = path.into();
        self.apply_sandbox_caps();
        self
    }

    fn apply_sandbox_caps(&mut self) {
        self.cli.caps = self.probed.clone();
        if self.sandbox_exec.is_file() {
            self.cli.caps.read_only = true;
            self.cli.caps.tool_allowlist = true;
        }
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
        if !self.sandbox_exec.is_file() {
            bail!(
                "agy cannot confine a job because {} is missing",
                self.sandbox_exec.display()
            );
        }
        self.cli.require("agy", &job, resume, "--conversation")?;
        let agy_args = build_args(&job, resume, &self.cli.caps);
        let home = user_home()?;
        let job_dir = std::fs::canonicalize(&job.workdir)
            .with_context(|| format!("job workdir {} cannot be resolved", job.workdir.display()))?;
        let agy_bin = std::fs::canonicalize(&self.cli.binary).with_context(|| {
            format!(
                "agy binary {} cannot be resolved",
                self.cli.binary.display()
            )
        })?;
        let job_s = utf8_path(&job_dir)?.to_string();
        let agy_s = utf8_path(&agy_bin)?.to_string();
        let profile = render_profile(&home, &job_s, &agy_s)?;
        let (dir, file) = allocate_profile(&job_dir, &profile)?;
        let mut args = vec!["-f".to_string(), utf8_path(&file)?.to_string(), agy_s];
        args.extend(agy_args);
        runner::launch(
            runner::Launch {
                label: "agy",
                binary: &self.sandbox_exec,
                args,
                workdir: &job_dir,
                extra_scrub: AGY_METERING_ENV_VARS,
                tap: self.cli.tap.as_deref(),
                timeout: job.timeout,
                schema: job.json_schema.as_ref(),
                cleanup_dir: Some(dir),
            },
            AgyParser::new(),
        )
        .await
    }
}

#[async_trait::async_trait]
impl AgentDriver for AgyDriver {
    fn id(&self) -> &'static str {
        "agy"
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
        runner::kill(h, "agy").await
    }
}

/// One model `agy models` listed. `efforts` are the suffixes seen for that
/// base id, in the order of `levels`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedModel {
    pub id: String,
    pub label: String,
    pub efforts: Vec<Effort>,
}

/// Parse `agy models` text into base model ids.
///
/// Lines are `<id>\t<label>`. The status line `Fetching available models...`
/// is not a model. A trailing `-<level>` is stripped only when that tail is
/// one of `levels` (the `--help` list). The whole id stays when the tail is
/// not a reported level, so `-high` does not eat `-xhigh`. Passing the
/// suffixed id together with `--effort` makes agy reject the job, so the
/// driver keeps the base id. An id that could not be passed as `--model` is
/// dropped. A line with no reported suffix is a model with empty efforts.
pub fn parse_models(text: &str, levels: &[Effort]) -> Vec<ListedModel> {
    let mut models: Vec<ListedModel> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Fetching available models") {
            continue;
        }
        let Some((id, label)) = line.split_once('\t') else {
            continue;
        };
        let (base, level) = split_reported_suffix(id.trim(), levels);
        let label = strip_level_label(label.trim(), levels);
        if label.is_empty() || ModelId::parse(base).is_err() {
            continue;
        }
        if let Some(existing) = models.iter_mut().find(|model| model.id == base) {
            if let Some(level) = level {
                if !existing.efforts.iter().any(|have| have == &level) {
                    existing.efforts.push(level);
                }
            }
            continue;
        }
        models.push(ListedModel {
            id: base.to_string(),
            label: label.to_string(),
            efforts: level.into_iter().collect(),
        });
    }
    for model in &mut models {
        model.efforts = order_like(levels, &model.efforts);
    }
    models
}

/// The tail after the last `-` is the effort only when `levels` names it.
/// Matching the whole tail, not `ends_with("-high")`, keeps `xhigh` intact
/// when `high` is also a reported level.
fn split_reported_suffix<'a>(id: &'a str, levels: &[Effort]) -> (&'a str, Option<Effort>) {
    let Some((base, tail)) = id.rsplit_once('-') else {
        return (id, None);
    };
    if base.is_empty() {
        return (id, None);
    }
    match levels.iter().find(|level| level.as_str() == tail) {
        Some(level) => (base, Some(level.clone())),
        None => (id, None),
    }
}

fn strip_level_label<'a>(label: &'a str, levels: &[Effort]) -> &'a str {
    let Some(open) = label.rfind(" (") else {
        return label;
    };
    let Some(inside) = label[open + 2..].strip_suffix(')') else {
        return label;
    };
    let known = levels
        .iter()
        .any(|level| level.as_str() == inside.to_ascii_lowercase());
    if !known {
        return label;
    }
    let base = label[..open].trim_end();
    if base.is_empty() {
        label
    } else {
        base
    }
}

fn order_like(levels: &[Effort], seen: &[Effort]) -> Vec<Effort> {
    levels
        .iter()
        .filter(|level| seen.iter().any(|have| have == *level))
        .cloned()
        .collect()
}

/// argv for one job, a pure function of the job and the probed capabilities.
/// Permission prompts are skipped. The sandbox, not `--mode`, is the guard,
/// so `accept-edits` is not passed.
///
/// `-p` takes the prompt as its value, so a prompt that looks like a flag is
/// consumed as the value and cannot be read as an option.
pub fn build_args(job: &JobSpec, resume: Option<&str>, caps: &Capabilities) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        shaped_prompt(job, caps),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--dangerously-skip-permissions".to_string(),
    ];
    if let Some(schema) = job.json_schema.as_ref().filter(|_| caps.structured_output) {
        args.push("--json-schema".to_string());
        args.push(schema.to_string());
    }
    if let Some(conversation) = resume {
        args.push("--conversation".to_string());
        args.push(conversation.to_string());
    }
    crate::types::push_selection(&mut args, job, caps);
    args
}

/// The profile text for one job. `job` is the canonical workdir. `binary` is
/// the canonical agy path and replaces `@HOME@/.local/bin/agy` before `@HOME@`
/// is replaced, so the exec allow rule names the binary the driver runs.
///
/// A quote or a backslash is refused. Either one would break out of the SBPL
/// string the path is placed in.
pub fn render_profile(home: &str, job: &str, binary: &str) -> anyhow::Result<String> {
    let home = home.trim_end_matches('/');
    for (label, value) in [("home", home), ("job", job), ("binary", binary)] {
        if value.is_empty() {
            bail!("the {label} path is empty, so it cannot go in the sandbox profile");
        }
        if value.contains('"') {
            bail!("the {label} path contains a quote, so it cannot go in the sandbox profile");
        }
        if value.contains('\\') {
            bail!("the {label} path contains a backslash, so it cannot go in the sandbox profile");
        }
    }
    if !PROFILE_TEMPLATE.contains(AGY_EXEC_PLACEHOLDER) {
        bail!("sandbox profile template is missing the agy exec literal");
    }
    let text = PROFILE_TEMPLATE
        .replace(AGY_EXEC_PLACEHOLDER, binary)
        .replace("@HOME@", home)
        .replace("@JOB@", job);
    if text.contains("@HOME@") || text.contains("@JOB@") {
        bail!("sandbox profile still contains a placeholder");
    }
    Ok(text)
}

/// Full argv of `sandbox-exec -f <profile> <agy> <agy args...>`.
pub fn sandbox_argv(
    sandbox_exec: &str,
    profile: &str,
    agy: &str,
    agy_args: &[String],
) -> Vec<String> {
    let mut argv = vec![
        sandbox_exec.to_string(),
        "-f".to_string(),
        profile.to_string(),
        agy.to_string(),
    ];
    argv.extend(agy_args.iter().cloned());
    argv
}

fn user_home() -> anyhow::Result<String> {
    let home = std::env::var("HOME")
        .map_err(|_| anyhow::anyhow!("HOME is not set, so agy cannot build a sandbox profile"))?;
    let home = home.trim_end_matches('/').to_string();
    if home.is_empty() {
        bail!("HOME is empty, so agy cannot build a sandbox profile");
    }
    Ok(home)
}

fn utf8_path(path: &Path) -> anyhow::Result<&str> {
    path.to_str()
        .context("a path in the sandbox profile is not utf-8")
}

fn allocate_profile(workdir: &Path, text: &str) -> anyhow::Result<(PathBuf, PathBuf)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "tutordocs-agy-sb-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir)
        .with_context(|| format!("cannot create sandbox profile dir {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let file = dir.join("profile.sb");
    if let Err(err) = std::fs::write(&file, text) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(err).context("cannot write the sandbox profile");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
    }
    let canon_dir = std::fs::canonicalize(&dir)?;
    let canon_work = std::fs::canonicalize(workdir)?;
    if canon_dir.starts_with(&canon_work) {
        let _ = std::fs::remove_dir_all(&dir);
        bail!("sandbox profile directory lives inside the job workdir");
    }
    Ok((dir, file))
}

/// Parser for agy's `{"event": ...}` NDJSON stream: one `init`, any number of
/// `step_update`s, one `result`. Unknown events are ignored so an update that
/// adds one does not break the stream.
#[derive(Default)]
pub struct AgyParser {
    session_emitted: bool,
    tools_reported: HashSet<u64>,
    in_text: bool,
}

impl AgyParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse one stdout line into zero or more events.
    pub fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        match v.get("event").and_then(Value::as_str).unwrap_or("") {
            "init" => self.on_init(&v),
            "step_update" => self.on_step(&v["step_update"]),
            "result" => {
                let mut out = self.boundary().into_iter().collect::<Vec<_>>();
                out.extend(on_result(&v["result"]));
                out
            }
            _ => Vec::new(),
        }
    }

    /// Close the open assistant text, once, when a later step starts.
    fn boundary(&mut self) -> Option<AgentEvent> {
        if self.in_text {
            self.in_text = false;
            Some(AgentEvent::MessageBoundary)
        } else {
            None
        }
    }

    fn on_init(&mut self, v: &Value) -> Vec<AgentEvent> {
        match v.get("conversation_id").and_then(Value::as_str) {
            Some(id) if !id.is_empty() && !self.session_emitted => {
                self.session_emitted = true;
                vec![AgentEvent::Session { id: id.to_string() }]
            }
            _ => Vec::new(),
        }
    }

    fn on_step(&mut self, step: &Value) -> Vec<AgentEvent> {
        match step.get("step_type").and_then(Value::as_str).unwrap_or("") {
            "agent_response" => step
                .get("text_delta")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .map(|t| {
                    self.in_text = true;
                    vec![AgentEvent::TextDelta(t.to_string())]
                })
                .unwrap_or_default(),
            "tool" => {
                let mut out = self.boundary().into_iter().collect::<Vec<_>>();
                let name = step
                    .get("tool_name")
                    .or_else(|| step.pointer("/tool_info/name"))
                    .and_then(Value::as_str);
                let index = step.get("step_index").and_then(Value::as_u64);
                if let (Some(name), Some(index)) = (name, index) {
                    if self.tools_reported.insert(index) {
                        let detail = step
                            .pointer("/tool_info/parameters")
                            .and_then(|params| crate::logline::tool_detail(name, params));
                        out.push(AgentEvent::ToolUse {
                            name: name.to_string(),
                            detail,
                        });
                    }
                }
                out
            }
            _ => self.boundary().into_iter().collect(),
        }
    }
}

fn on_result(result: &Value) -> Vec<AgentEvent> {
    let status = result.get("status").and_then(Value::as_str);
    let error = result
        .get("error")
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty());
    match (status, error) {
        (Some("SUCCESS"), None) => {
            let value = match result.get("structured_output") {
                Some(structured) if !structured.is_null() => structured.clone(),
                _ => json_or_text(result.get("response").and_then(Value::as_str).unwrap_or("")),
            };
            vec![AgentEvent::Result { value }]
        }
        (_, Some(error)) => vec![AgentEvent::Error(error.to_string())],
        (Some(status), None) => vec![AgentEvent::Error(format!(
            "agy run ended with status {status}"
        ))],
        (None, None) => vec![AgentEvent::Error("agy result carried no status".into())],
    }
}

impl LineParser for AgyParser {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        AgyParser::parse_line(self, line)
    }
}
