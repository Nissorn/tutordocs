//! Ticket 11 spike: exactly ONE small real `qodercli -p` job through the real
//! QoderDriver, with the metering env scrubbed. One job proves three things:
//! the browser-login auth path, the stream-json event shape, and the
//! prompt-enforced structured output (Qoder's help lists no `--json-schema`).
//!
//! It is a one-shot: it refuses to run when the raw tap from an earlier run is
//! still on disk. Delete the tap on purpose to rerun.
//!
//! ```text
//! cargo run -p tutordocs-agent --example spike11_qoder -- /path/to/qoder /path/to/raw-tap.ndjson
//! ```
//!
//! Prints variable names only, never values.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tutordocs_agent::env::{child_env_with, METERING_ENV_VARS, QODER_METERING_ENV_VARS};
use tutordocs_agent::{AgentDriver, AgentEvent, JobSpec, QoderDriver};

fn names_present(is_set: impl Fn(&str) -> bool) -> Vec<String> {
    METERING_ENV_VARS
        .iter()
        .chain(QODER_METERING_ENV_VARS)
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
    let binary = args.next().unwrap_or_else(|| "qoder".into());
    let tap = PathBuf::from(args.next().ok_or_else(|| {
        anyhow::anyhow!("usage: spike11_qoder <qoder binary> <raw tap path outside the repo>")
    })?);
    if tap.exists() {
        anyhow::bail!(
            "{} exists: the qoder spike already ran. Delete it only on purpose.",
            tap.display()
        );
    }
    let workdir =
        std::env::temp_dir().join(format!("tutordocs-spike11-qoder-{}", std::process::id()));
    std::fs::create_dir_all(&workdir)?;

    println!("== spike11 qoder: one real qodercli -p job ==");
    let version = std::process::Command::new(&binary)
        .arg("--version")
        .output()?;
    println!(
        "binary: {binary}\nversion: {}",
        String::from_utf8_lossy(&version.stdout).trim()
    );

    let parent_metering = names_present(|n| std::env::var_os(n).is_some());
    let child = child_env_with(QODER_METERING_ENV_VARS);
    let child_metering = names_present(|n| child.contains_key(n));
    let mut qoder_vars: Vec<String> = std::env::vars()
        .map(|(k, _)| k)
        .filter(|k| k.starts_with("QODER_"))
        .collect();
    qoder_vars.sort();
    println!("parent_metering_vars_set: {parent_metering:?}");
    println!("child_metering_vars_set: {child_metering:?}");
    println!("keyless_child_env: {}", child_metering.is_empty());
    println!("parent_qoder_vars_inherited (kept, not metering): {qoder_vars:?}");

    let driver = QoderDriver::new(&binary)?.with_ndjson_tap(&tap);
    println!("capabilities: {:?}", driver.capabilities());

    let schema = json!({
        "type": "object",
        "properties": {"answer": {"type": "integer"}},
        "required": ["answer"],
        "additionalProperties": false
    });
    let mut job = JobSpec::new("Set answer to the integer 7.", &workdir);
    job.json_schema = Some(schema);
    job.timeout = Some(Duration::from_secs(120));

    let started = Instant::now();
    let handle = driver.start(job).await?;
    println!("pid: {:?}", handle.pid);
    let mut stream = driver.events(&handle);
    let (mut session, mut result, mut errors) = (None, None, Vec::new());
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
    println!(
        "matches_expected_{{\"answer\":7}}: {}",
        result == Some(json!({"answer": 7}))
    );
    println!("driver_errors: {errors:?}");
    println!("wall_clock_ms: {wall_clock_ms}");
    println!(
        "tap_line_types: {:?}",
        lines.iter().map(|v| field(v, "type")).collect::<Vec<_>>()
    );
    if let Some(init) = init {
        for key in [
            "apiKeySource",
            "model",
            "permissionMode",
            "cwd",
            "qodercli_version",
        ] {
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
            "total_credits",
            "usage",
            "terminal_reason",
            "structured_output",
        ] {
            println!("result.{key}: {}", field(r, key));
        }
        println!("result_line: {r}");
    } else {
        println!("result_line: (no result line in the tap)");
    }
    println!("raw_ndjson: {}", tap.display());

    if result.is_none() || !errors.is_empty() || !child_metering.is_empty() {
        anyhow::bail!("spike11 qoder did not complete cleanly; see the summary above");
    }
    Ok(())
}
