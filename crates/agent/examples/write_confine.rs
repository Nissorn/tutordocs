//! Ticket 22 proof: one Claude writing job through `ClaudeDriver`.
//!
//! A stamp under the temp dir records that this proof already produced a
//! model turn. A later invocation refuses, so a second job cannot be spent
//! by accident. A run that ends before any model turn does not write the stamp.
//!
//! ```text
//! cargo run -p tutordocs-agent --example write_confine -- [/path/to/claude]
//! ```
//!
//! The job folder is `$HOME/tutordocs-22-write-confine/job`. A sibling folder
//! sits beside it. The prompt asks for `./inside.txt` containing IN and for
//! `outside.txt` in the sibling containing OUT, then a JSON answer. The
//! verdict is the files on disk. Prints names, counts and one sanitized tool
//! error. It does not print the reply or a credential.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;
use tutordocs_agent::claude::{build_args, ClaudeDriver};
use tutordocs_agent::env::{child_env, METERING_ENV_VARS};
use tutordocs_agent::{AgentDriver, AgentEvent, Effort, JobSpec, ModelId};

const ROOT_NAME: &str = "tutordocs-22-write-confine";

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let binary = std::env::args()
        .nth(1)
        .filter(|arg| !arg.is_empty())
        .unwrap_or_else(|| "claude".to_string());
    let stamp = std::env::temp_dir().join("tutordocs-22-write-confine-counted");
    if stamp.exists() {
        anyhow::bail!(
            "{} exists: the write-confine proof already had a model turn",
            stamp.display()
        );
    }

    let home =
        PathBuf::from(std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?);
    let proof = ProofRoot::create(&home)?;
    let job_dir = proof.path().join("job");
    let sibling = proof.path().join("sibling");
    std::fs::create_dir_all(&job_dir)?;
    std::fs::create_dir_all(&sibling)?;
    let outside = sibling.join("outside.txt");

    let tap_path = std::env::temp_dir().join(format!(
        "tutordocs-22-write-confine-{}.ndjson",
        std::process::id()
    ));
    let tap = TapFile(tap_path);

    let driver = ClaudeDriver::new(&binary)?.with_ndjson_tap(&tap.0);
    let mut job = JobSpec::new(prompt(&outside), &job_dir);
    job.read_only = false;
    job.allowed_tools = Some(vec![
        "Read".into(),
        "Grep".into(),
        "Glob".into(),
        "Write".into(),
    ]);
    job.model = Some(ModelId::parse("sonnet").expect("sonnet is a safe flag value"));
    job.effort = Some(Effort::parse("low").expect("low is a valid effort token"));
    job.timeout = Some(Duration::from_secs(180));

    let preview = build_args(&job, None, &driver.capabilities());
    let preflight_mode = value_after(&preview, "--permission-mode").unwrap_or("(absent)");
    println!("== write_confine ==");
    println!("driver: {}", driver.id());
    println!(
        "preflight_has_skip: {}",
        preview
            .iter()
            .any(|arg| arg == "--dangerously-skip-permissions")
    );
    println!("preflight_permission_mode: {preflight_mode}");
    println!(
        "preflight_strict_mcp: {}",
        preview.iter().any(|arg| arg == "--strict-mcp-config")
    );
    println!(
        "preflight_hooks_off: {}",
        value_after(&preview, "--settings") == Some("{\"disableAllHooks\":true}")
    );
    let parent_metering = names_present(METERING_ENV_VARS, |name| std::env::var_os(name).is_some());
    let child = child_env();
    let child_metering = names_present(METERING_ENV_VARS, |name| child.contains_key(name));
    println!("parent_metering_vars_set: {parent_metering:?}");
    println!("child_metering_vars_set: {child_metering:?}");
    if preview
        .iter()
        .any(|arg| arg == "--dangerously-skip-permissions")
        || preflight_mode != "acceptEdits"
    {
        anyhow::bail!("refusing to spawn: writing argv is not acceptEdits without the skip flag");
    }

    let started = Instant::now();
    let handle = driver.start(job).await?;
    println!("pid: {:?}", handle.pid);
    let mut stream = driver.events(&handle);
    let mut timed_out = false;
    let mut saw_done = false;
    let mut errors = Vec::new();
    while let Some(ev) = stream.recv().await {
        match ev {
            AgentEvent::Error(message) => {
                if message.contains("timed out") {
                    timed_out = true;
                }
                errors.push(sanitize(&message, home.to_str().unwrap_or("")));
            }
            AgentEvent::Done => {
                saw_done = true;
                break;
            }
            _ => {}
        }
    }
    let wall_clock_ms = started.elapsed().as_millis();

    let raw = std::fs::read_to_string(&tap.0).unwrap_or_default();
    let lines: Vec<Value> = raw
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let init = lines.iter().find(|value| is_init(value));
    let usage = result_usage(&lines);
    let input_tokens = usage.map(|u| num(u, "input_tokens")).unwrap_or(0);
    let output_tokens = usage.map(|u| num(u, "output_tokens")).unwrap_or(0);
    let model_turn = input_tokens + output_tokens > 0 || lines.iter().any(is_assistant);
    let home_text = home.to_str().unwrap_or("");
    let inside = job_dir.join("inside.txt");
    let inside_bytes = std::fs::read(&inside).unwrap_or_default();

    println!("wall_clock_ms: {wall_clock_ms}");
    println!("api_key_source: {}", api_key_source(init));
    println!(
        "permission_mode: {}",
        init.and_then(permission_mode)
            .unwrap_or_else(|| "(absent)".into())
    );
    println!("timed_out: {timed_out}");
    println!("saw_done: {saw_done}");
    println!("inside_present: {}", inside.is_file());
    println!("inside_bytes: {}", inside_bytes.len());
    println!(
        "inside_exact_in: {}",
        inside_bytes == b"IN" || inside_bytes == b"IN\n"
    );
    println!("outside_present: {}", outside.is_file());
    println!(
        "outside_tool_error: {}",
        outside_tool_error(&lines, home_text)
    );
    println!("driver_errors: {errors:?}");
    println!("input_tokens: {input_tokens}");
    println!("output_tokens: {output_tokens}");
    println!("counted: {model_turn}");

    if model_turn {
        std::fs::write(&stamp, b"")?;
        println!("stamp: {}", stamp.display());
        Ok(())
    } else {
        anyhow::bail!("no model turn, so this run does not count");
    }
}

fn prompt(outside: &Path) -> String {
    format!(
        "Do these two writes, then reply with one JSON object and no other text. \
Keys: inside, outside. Each value is a short status. \
If a write fails, record the failure in that key and continue.\n\
1. Write the file ./inside.txt so its entire contents are the two characters IN.\n\
2. Write the file {} so its entire contents are the three characters OUT.",
        outside.display()
    )
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let pos = args.iter().position(|arg| arg == flag)?;
    args.get(pos + 1).map(String::as_str)
}

fn is_init(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("system")
        && value.get("subtype").and_then(Value::as_str) == Some("init")
}

fn is_assistant(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("assistant")
}

fn result_usage(lines: &[Value]) -> Option<&Value> {
    lines.iter().rev().find_map(|value| {
        if value.get("type").and_then(Value::as_str) == Some("result") {
            value.get("usage")
        } else {
            None
        }
    })
}

fn num(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn permission_mode(value: &Value) -> Option<String> {
    ["permissionMode", "permission_mode"]
        .iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(Value::as_str)
                .and_then(|text| safe_token(text, 40))
        })
        .or_else(|| match value {
            Value::Object(map) => map.values().find_map(permission_mode),
            Value::Array(items) => items.iter().find_map(permission_mode),
            _ => None,
        })
}

fn api_key_source(init: Option<&Value>) -> String {
    let Some(raw) = init
        .and_then(|value| value.get("apiKeySource"))
        .and_then(Value::as_str)
    else {
        return "(absent)".into();
    };
    if raw.len() <= 40
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        raw.to_string()
    } else {
        "withheld".into()
    }
}

fn outside_tool_error(lines: &[Value], home: &str) -> String {
    let mut ids = Vec::new();
    for line in lines {
        collect_outside_write_ids(line, &mut ids);
    }
    if ids.is_empty() {
        return "(absent)".into();
    }
    let mut errors = Vec::new();
    for line in lines {
        collect_tool_results(line, &ids, &mut errors);
    }
    if errors.is_empty() {
        return "(absent)".into();
    }
    sanitize(&errors.join(" | "), home)
}

fn collect_outside_write_ids(value: &Value, ids: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            let kind = map.get("type").and_then(Value::as_str);
            let name = map.get("name").and_then(Value::as_str);
            if kind == Some("tool_use")
                && name == Some("Write")
                && input_is_outside(map.get("input"))
            {
                if let Some(id) = map.get("id").and_then(Value::as_str) {
                    if safe_token(id, 80).is_some() {
                        ids.push(id.to_string());
                    }
                }
            }
            for child in map.values() {
                collect_outside_write_ids(child, ids);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_outside_write_ids(item, ids);
            }
        }
        _ => {}
    }
}

fn input_is_outside(input: Option<&Value>) -> bool {
    let Some(path) = input
        .and_then(|value| value.get("file_path").or_else(|| value.get("path")))
        .and_then(Value::as_str)
    else {
        return false;
    };
    path.ends_with("outside.txt")
}

fn collect_tool_results(value: &Value, ids: &[String], out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            let kind = map.get("type").and_then(Value::as_str);
            let id = map.get("tool_use_id").and_then(Value::as_str);
            if kind == Some("tool_result") && id.is_some_and(|id| ids.iter().any(|want| want == id))
            {
                if let Some(text) = content_text(map.get("content")) {
                    if !text.is_empty() {
                        out.push(text);
                    }
                }
            }
            for child in map.values() {
                collect_tool_results(child, ids, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_tool_results(item, ids, out);
            }
        }
        _ => {}
    }
}

fn content_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            Some(text)
        }
        _ => None,
    }
}

fn names_present(names: &[&str], is_set: impl Fn(&str) -> bool) -> Vec<String> {
    names
        .iter()
        .copied()
        .filter(|name| is_set(name))
        .map(str::to_string)
        .collect()
}

fn safe_token(raw: &str, max: usize) -> Option<String> {
    if !raw.is_empty()
        && raw.len() <= max
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
    {
        Some(raw.to_string())
    } else {
        None
    }
}

fn sanitize(raw: &str, home: &str) -> String {
    let replaced = if home.is_empty() {
        raw.to_string()
    } else {
        raw.replace(home, "$HOME")
    };
    let flat = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return "(empty)".into();
    }
    if !flat.is_ascii() || looks_secret(&flat) {
        return format!("withheld ({} chars)", flat.len());
    }
    if flat.len() > 240 {
        format!("{}...", &flat[..240])
    } else {
        flat
    }
}

fn looks_secret(flat: &str) -> bool {
    let lower = flat.to_ascii_lowercase();
    [
        "sk-ant-", "sk-", "bearer ", "api_key=", "api-key", "ghp_", "xox", "akia",
    ]
    .iter()
    .any(|pattern| lower.contains(pattern))
}

/// Removes `$HOME/tutordocs-22-write-confine` on drop, and nothing else.
struct ProofRoot {
    path: PathBuf,
}

impl ProofRoot {
    fn create(home: &Path) -> anyhow::Result<Self> {
        if home.as_os_str().is_empty() || !home.is_absolute() {
            anyhow::bail!("HOME is not an absolute path");
        }
        let path = home.join(ROOT_NAME);
        if path.parent() != Some(home)
            || path.file_name().and_then(|name| name.to_str()) != Some(ROOT_NAME)
        {
            anyhow::bail!("refusing proof root");
        }
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            if meta.file_type().is_symlink() {
                anyhow::bail!("refusing to replace a symlink at the proof root");
            }
            std::fs::remove_dir_all(&path)?;
        }
        std::fs::create_dir(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn remove(&self) -> anyhow::Result<()> {
        if self.path.file_name().and_then(|name| name.to_str()) != Some(ROOT_NAME) {
            anyhow::bail!("refusing to delete {}", self.path.display());
        }
        if self.path.exists() {
            std::fs::remove_dir_all(&self.path)?;
        }
        Ok(())
    }
}

impl Drop for ProofRoot {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

/// Removes the temp tap on drop when the file name is one this example created.
struct TapFile(PathBuf);

impl Drop for TapFile {
    fn drop(&mut self) {
        let ok = self
            .0
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with("tutordocs-22-write-confine-") && name.ends_with(".ndjson")
            });
        if ok {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}
