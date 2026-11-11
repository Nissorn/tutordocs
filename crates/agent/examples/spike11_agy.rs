//! Ticket 11 spike: exactly ONE small real `agy -p` job through the real
//! AgyDriver, with the metering env scrubbed. One job proves three things: the
//! cached-Google-login auth path in a non-TTY child (the reported hang), the
//! stream-json event shape, and system shaping through the prompt body (agy
//! has no system-prompt flag).
//!
//! It is a one-shot: it refuses to run when the raw tap from an earlier run is
//! still on disk. Delete the tap on purpose to rerun. This example sets a
//! two-minute limit, so a hang ends in a reported timeout.
//!
//! ```text
//! cargo run -p tutordocs-agent --example spike11_agy -- /path/to/agy /path/to/raw-tap.ndjson
//! ```
//!
//! Prints variable names only, never values.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::Value;
use tutordocs_agent::env::{child_env_with, AGY_METERING_ENV_VARS, METERING_ENV_VARS};
use tutordocs_agent::{AgentDriver, AgentEvent, AgyDriver, JobSpec};

fn names_present(is_set: impl Fn(&str) -> bool) -> Vec<String> {
    METERING_ENV_VARS
        .iter()
        .chain(AGY_METERING_ENV_VARS)
        .filter(|n| is_set(n))
        .map(|n| (*n).to_string())
        .collect()
}

fn field(v: &Value, key: &str) -> String {
    v.get(key).map_or("(absent)".to_string(), Value::to_string)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let binary = args.next().unwrap_or_else(|| "agy".into());
    let tap = PathBuf::from(args.next().ok_or_else(|| {
        anyhow::anyhow!("usage: spike11_agy <agy binary> <raw tap path outside the repo>")
    })?);
    if tap.exists() {
        anyhow::bail!(
            "{} exists: the agy spike already ran. Delete it only on purpose.",
            tap.display()
        );
    }
    let workdir =
        std::env::temp_dir().join(format!("tutordocs-spike11-agy-{}", std::process::id()));
    std::fs::create_dir_all(&workdir)?;

    println!("== spike11 agy: one real agy -p job ==");
    let version = std::process::Command::new(&binary)
        .arg("--version")
        .output()?;
    println!(
        "binary: {binary}\nversion: {}",
        String::from_utf8_lossy(&version.stdout).trim()
    );

    let parent_metering = names_present(|n| std::env::var_os(n).is_some());
    let child = child_env_with(AGY_METERING_ENV_VARS);
    let child_metering = names_present(|n| child.contains_key(n));
    println!("parent_metering_vars_set: {parent_metering:?}");
    println!("child_metering_vars_set: {child_metering:?}");
    println!("keyless_child_env: {}", child_metering.is_empty());

    let driver = AgyDriver::new(&binary)?.with_ndjson_tap(&tap);
    println!("capabilities: {:?}", driver.capabilities());

    let mut job = JobSpec::new(
        "What is the secret word? Reply with that single word only.",
        &workdir,
    );
    job.system_append = Some("The secret word is pelican.".into());
    job.timeout = Some(Duration::from_secs(120));
    // agy has no confirmed read-only mode and refuses a read-only job; this
    // one runs edit-capable in its own temp workdir.
    job.read_only = false;

    let started = Instant::now();
    let handle = driver.start(job).await?;
    println!("pid: {:?}", handle.pid);
    let mut stream = driver.events(&handle);
    let (mut session, mut result, mut errors) = (None, None, Vec::new());
    let mut streamed = String::new();
    while let Some(ev) = stream.recv().await {
        println!("event: {ev:?}");
        match ev {
            AgentEvent::Session { id } => session = Some(id),
            AgentEvent::TextDelta(text) => streamed.push_str(&text),
            AgentEvent::Result { value } => result = Some(value),
            AgentEvent::Error(message) => errors.push(message),
            AgentEvent::Done => break,
            _ => {}
        }
    }
    let wall_clock_ms = started.elapsed().as_millis();
    let timed_out = errors.iter().any(|e| e.contains("timed out"));

    let raw = std::fs::read_to_string(&tap).unwrap_or_default();
    let lines: Vec<Value> = raw
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let result_line = lines.iter().find(|v| v["event"] == "result");

    println!("== summary ==");
    println!("session_id: {session:?}");
    println!("result: {result:?}");
    println!(
        "answer_contains_pelican: {}",
        result
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|s| s.to_lowercase().contains("pelican"))
    );
    println!(
        "streamed_text_equals_result_text (false means deltas doubled or differ): {}",
        result.as_ref().and_then(Value::as_str) == Some(streamed.as_str())
    );
    println!("driver_errors: {errors:?}");
    println!("timed_out: {timed_out}");
    println!("wall_clock_ms: {wall_clock_ms}");
    println!(
        "tap_event_names: {:?}",
        lines.iter().map(|v| field(v, "event")).collect::<Vec<_>>()
    );
    if let Some(init) = lines.iter().find(|v| v["event"] == "init") {
        for key in ["permission_mode", "model", "cwd"] {
            println!("init.{key}: {}", field(&init["init"], key));
        }
    } else {
        println!("init: (no init event in the tap)");
    }
    if let Some(r) = result_line {
        for key in ["status", "error", "duration_seconds", "num_turns", "usage"] {
            println!("result.{key}: {}", field(&r["result"], key));
        }
        println!("result_line: {r}");
    } else {
        println!("result_line: (no result event in the tap)");
    }
    println!("raw_ndjson: {}", tap.display());

    if result.is_none() || !errors.is_empty() || !child_metering.is_empty() {
        anyhow::bail!("spike11 agy did not complete cleanly; see the summary above");
    }
    Ok(())
}
