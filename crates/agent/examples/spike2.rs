//! Spike 2: exactly ONE small real `claude -p` job through the real driver,
//! with the metering env scrubbed. It is a one-shot: it refuses to run when
//! the raw tap from an earlier run is still on disk, so an accidental second
//! invocation cannot spend a second job. Delete the tap on purpose to rerun.
//!
//! ```text
//! cargo run -p tutordocs-agent --example spike2 -- /path/to/claude
//! ```
//!
//! Writes the raw NDJSON tap to `target/spike2-raw.ndjson`
//! and prints a summary: session id, result, keyless evidence and timing.
//! Prints variable names only, never values.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::Value;
use tutordocs_agent::env::{child_env, METERING_ENV_VARS};
use tutordocs_agent::{AgentDriver, AgentEvent, ClaudeDriver, JobSpec};

fn names_present(is_set: impl Fn(&str) -> bool, names: &[&str]) -> Vec<String> {
    names
        .iter()
        .filter(|n| is_set(n))
        .map(|n| (*n).to_string())
        .collect()
}

fn field(v: &Value, key: &str) -> String {
    v.get(key).map_or("(absent)".to_string(), Value::to_string)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let binary = std::env::args().nth(1).unwrap_or_else(|| "claude".into());
    let tap = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/agent sits two levels below the repo root")
        .join("target/spike2-raw.ndjson");
    if tap.exists() {
        anyhow::bail!(
            "{} exists: Spike 2 already ran. Delete it only on purpose.",
            tap.display()
        );
    }

    let workdir = std::env::temp_dir().join(format!("tutordocs-spike2-{}", std::process::id()));
    std::fs::create_dir_all(&workdir)?;

    println!("== spike2: one real claude -p job ==");
    let version = std::process::Command::new(&binary)
        .arg("--version")
        .output()?;
    println!(
        "binary: {binary}\nversion: {}",
        String::from_utf8_lossy(&version.stdout).trim()
    );

    let parent_metering = names_present(|n| std::env::var_os(n).is_some(), METERING_ENV_VARS);
    let child = child_env();
    let child_metering = names_present(|n| child.contains_key(n), METERING_ENV_VARS);
    let mut parent_claude_code: Vec<String> = std::env::vars()
        .map(|(k, _)| k)
        .filter(|k| k.starts_with("CLAUDE_CODE_"))
        .collect();
    parent_claude_code.sort();
    println!("parent_metering_vars_set: {parent_metering:?}");
    println!("child_metering_vars_set: {child_metering:?}");
    println!("keyless_child_env: {}", child_metering.is_empty());
    println!("parent_claude_code_vars_inherited: {parent_claude_code:?}");

    let driver = ClaudeDriver::new(&binary)?.with_ndjson_tap(&tap);
    println!("capabilities: {:?}", driver.capabilities());

    let mut job = JobSpec::new("Reply with exactly: ok", &workdir);
    job.timeout = Some(Duration::from_secs(120));

    let started = Instant::now();
    let handle = driver.start(job).await?;
    println!("pid: {:?}", handle.pid);
    let mut stream = driver.events(&handle);
    let mut session = None;
    let mut result = None;
    let mut errors = Vec::new();
    while let Some(ev) = stream.recv().await {
        println!("event: {ev:?}");
        match ev {
            AgentEvent::Session { id } => session = Some(id),
            AgentEvent::Result { value } => result = Some(value),
            AgentEvent::Error(message) => errors.push(message),
            AgentEvent::Done => break,
            _ => {}
        }
    }
    let wall_clock_ms = started.elapsed().as_millis();

    let raw = std::fs::read_to_string(&tap).unwrap_or_default();
    let lines: Vec<Value> = raw
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let init = lines
        .iter()
        .find(|v| v["type"] == "system" && v["subtype"] == "init");
    let result_line = lines.iter().find(|v| v["type"] == "result");

    println!("== summary ==");
    println!("session_id: {session:?}");
    println!("result: {result:?}");
    println!("driver_errors: {errors:?}");
    println!("wall_clock_ms: {wall_clock_ms}");
    if let Some(init) = init {
        for key in ["apiKeySource", "model", "permissionMode", "cwd"] {
            println!("init.{key}: {}", field(init, key));
        }
    } else {
        println!("init: (no system/init line in the tap)");
    }
    if let Some(r) = result_line {
        for key in [
            "subtype",
            "is_error",
            "duration_ms",
            "duration_api_ms",
            "num_turns",
            "total_cost_usd",
            "usage",
            "terminal_reason",
        ] {
            println!("result.{key}: {}", field(r, key));
        }
        println!("result_line: {r}");
    } else {
        println!("result_line: (no result line in the tap)");
    }
    println!("raw_ndjson: {}", tap.display());

    if result.is_none() || !errors.is_empty() || !child_metering.is_empty() {
        anyhow::bail!("spike2 did not complete cleanly; see the summary above");
    }
    Ok(())
}
