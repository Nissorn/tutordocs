//! App control socket and protocol (`tutordocs-ctl`), gated behind `e2e-control`.

#[cfg(feature = "e2e-control")]
use std::collections::HashMap;
#[cfg(feature = "e2e-control")]
use std::ffi::CString;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
#[cfg(feature = "e2e-control")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(feature = "e2e-control")]
use std::sync::{mpsc, Mutex};
use std::time::Duration;
#[cfg(feature = "e2e-control")]
use std::time::Instant;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value};

pub const DEFAULT_WAIT_TIMEOUT_MS: u64 = 10_000;
#[cfg(feature = "e2e-control")]
const DEFAULT_CMD_TIMEOUT_MS: u64 = 15_000;
#[cfg(feature = "e2e-control")]
const WAIT_SLICE_MS: u64 = 350;

/// Whether the `e2e-control` feature is compiled into this build.
pub const fn is_enabled() -> bool {
    cfg!(feature = "e2e-control")
}

/// Wire request sent as one JSON line over the control socket: `{id, cmd, args}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: Value,
    pub cmd: String,
    #[serde(default)]
    pub args: Value,
}

/// Wire reply sent as one JSON line over the control socket:
/// `{id, ok: true, value}` or `{id, ok: false, error}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub id: Value,
    pub result: Result<Value, String>,
}

impl Reply {
    pub fn ok(id: Value, value: Value) -> Self {
        Self {
            id,
            result: Ok(value),
        }
    }

    pub fn err(id: Value, error: impl Into<String>) -> Self {
        Self {
            id,
            result: Err(error.into()),
        }
    }
}

impl Serialize for Reply {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireOk<'a> {
            id: &'a Value,
            ok: bool,
            value: &'a Value,
        }
        #[derive(Serialize)]
        struct WireErr<'a> {
            id: &'a Value,
            ok: bool,
            error: &'a str,
        }
        match &self.result {
            Ok(value) => WireOk {
                id: &self.id,
                ok: true,
                value,
            }
            .serialize(serializer),
            Err(error) => WireErr {
                id: &self.id,
                ok: false,
                error,
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Reply {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            id: Value,
            ok: bool,
            #[serde(default)]
            value: Value,
            #[serde(default)]
            error: Option<String>,
        }
        let wire = Wire::deserialize(deserializer)?;
        if wire.ok {
            Ok(Reply::ok(wire.id, wire.value))
        } else {
            Ok(Reply::err(
                wire.id,
                wire.error.unwrap_or_else(|| "unknown error".to_string()),
            ))
        }
    }
}

/// Typed command parsed at the boundary from either CLI args or wire JSON `Request`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CtlCommand {
    WaitFor {
        selector: String,
        timeout_ms: u64,
    },
    Click {
        selector: String,
    },
    Type {
        selector: String,
        text: String,
    },
    Press {
        key: String,
    },
    Text {
        selector: String,
    },
    Count {
        selector: String,
    },
    Attr {
        selector: String,
        name: String,
    },
    SelectText {
        selector: String,
        substring: String,
    },
    Drop {
        paths: Vec<String>,
    },
    Snapshot {
        path: String,
    },
    Eval {
        js: String,
    },
    Resize {
        width: u32,
        height: u32,
    },
    /// Pins the window's appearance to `light` or `dark`, or back to `system`,
    /// so a replay can put the page's `prefers-color-scheme` against a forced theme.
    Appearance {
        mode: String,
    },
}

/// The window appearances `Appearance` accepts.
fn appearance_mode(raw: &str) -> Result<String, String> {
    match raw {
        "light" | "dark" | "system" => Ok(raw.to_string()),
        other => Err(format!(
            "appearance must be light, dark or system, not {other}"
        )),
    }
}

impl CtlCommand {
    pub fn from_cli_args(args: &[String]) -> Result<Self, String> {
        let Some((cmd, rest)) = args.split_first() else {
            return Err("missing command name".to_string());
        };
        Self::parse_list(cmd, rest)
    }

    pub fn from_request(req: &Request) -> Result<Self, String> {
        match &req.args {
            Value::Array(items) => {
                let mut strings = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        Value::String(s) => strings.push(s.clone()),
                        Value::Number(n) => strings.push(n.to_string()),
                        Value::Bool(b) => strings.push(b.to_string()),
                        _ => return Err(format!("invalid argument in array for {}", req.cmd)),
                    }
                }
                Self::parse_list(&req.cmd, &strings)
            }
            Value::Object(map) => Self::parse_map(&req.cmd, map),
            Value::Null => Self::parse_list(&req.cmd, &[]),
            Value::String(single) => Self::parse_list(&req.cmd, std::slice::from_ref(single)),
            _ => Err(format!("invalid args shape for {}", req.cmd)),
        }
    }

    fn parse_list(cmd: &str, rest: &[String]) -> Result<Self, String> {
        match cmd {
            "wait-for" => {
                if rest.is_empty() {
                    return Err("wait-for requires <selector> [--timeout ms]".to_string());
                }
                let selector = rest[0].clone();
                let mut timeout_ms = DEFAULT_WAIT_TIMEOUT_MS;
                let mut i = 1;
                while i < rest.len() {
                    let arg = &rest[i];
                    if arg == "--timeout" {
                        let val = rest.get(i + 1).ok_or_else(|| {
                            "wait-for --timeout requires a value in ms".to_string()
                        })?;
                        timeout_ms = val
                            .parse::<u64>()
                            .map_err(|_| format!("invalid --timeout value: {val}"))?;
                        i += 2;
                    } else if let Some(val) = arg.strip_prefix("--timeout=") {
                        timeout_ms = val
                            .parse::<u64>()
                            .map_err(|_| format!("invalid --timeout value: {val}"))?;
                        i += 1;
                    } else {
                        return Err(format!("unexpected argument to wait-for: {arg}"));
                    }
                }
                Ok(Self::WaitFor {
                    selector,
                    timeout_ms,
                })
            }
            "click" => {
                if rest.len() != 1 {
                    return Err("click requires exactly <selector>".to_string());
                }
                Ok(Self::Click {
                    selector: rest[0].clone(),
                })
            }
            "type" => {
                if rest.len() < 2 {
                    return Err("type requires <selector> <text>".to_string());
                }
                Ok(Self::Type {
                    selector: rest[0].clone(),
                    text: rest[1..].join(" "),
                })
            }
            "press" => {
                if rest.len() != 1 {
                    return Err("press requires exactly <key>".to_string());
                }
                Ok(Self::Press {
                    key: rest[0].clone(),
                })
            }
            "text" => {
                if rest.len() != 1 {
                    return Err("text requires exactly <selector>".to_string());
                }
                Ok(Self::Text {
                    selector: rest[0].clone(),
                })
            }
            "count" => {
                if rest.len() != 1 {
                    return Err("count requires exactly <selector>".to_string());
                }
                Ok(Self::Count {
                    selector: rest[0].clone(),
                })
            }
            "attr" => {
                if rest.len() != 2 {
                    return Err("attr requires <selector> <name>".to_string());
                }
                Ok(Self::Attr {
                    selector: rest[0].clone(),
                    name: rest[1].clone(),
                })
            }
            "select-text" => {
                if rest.len() < 2 {
                    return Err("select-text requires <selector> <substring>".to_string());
                }
                Ok(Self::SelectText {
                    selector: rest[0].clone(),
                    substring: rest[1..].join(" "),
                })
            }
            "drop" => {
                if rest.is_empty() {
                    return Err("drop requires at least one path".to_string());
                }
                Ok(Self::Drop {
                    paths: rest.to_vec(),
                })
            }
            "snapshot" => {
                if rest.len() != 1 {
                    return Err("snapshot requires <file.png>".to_string());
                }
                Ok(Self::Snapshot {
                    path: rest[0].clone(),
                })
            }
            "eval" => {
                if rest.is_empty() {
                    return Err("eval requires <js>".to_string());
                }
                Ok(Self::Eval { js: rest.join(" ") })
            }
            "resize" => {
                if rest.len() != 2 {
                    return Err("resize requires <width> <height>".to_string());
                }
                let width = rest[0]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid width: {}", rest[0]))?;
                let height = rest[1]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid height: {}", rest[1]))?;
                Ok(Self::Resize { width, height })
            }
            "appearance" => {
                if rest.len() != 1 {
                    return Err("appearance requires <light|dark|system>".to_string());
                }
                Ok(Self::Appearance {
                    mode: appearance_mode(&rest[0])?,
                })
            }
            other => Err(format!("unknown command: {other}")),
        }
    }

    fn parse_map(cmd: &str, map: &serde_json::Map<String, Value>) -> Result<Self, String> {
        let get_str = |key: &str| -> Result<String, String> {
            map.get(key)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("{cmd} requires string field '{key}'"))
        };
        match cmd {
            "wait-for" => {
                let selector = get_str("selector")?;
                let timeout_ms = map
                    .get("timeout")
                    .or_else(|| map.get("timeoutMs"))
                    .or_else(|| map.get("timeout_ms"))
                    .and_then(Value::as_u64)
                    .unwrap_or(DEFAULT_WAIT_TIMEOUT_MS);
                Ok(Self::WaitFor {
                    selector,
                    timeout_ms,
                })
            }
            "click" => Ok(Self::Click {
                selector: get_str("selector")?,
            }),
            "type" => Ok(Self::Type {
                selector: get_str("selector")?,
                text: get_str("text")?,
            }),
            "press" => Ok(Self::Press {
                key: get_str("key")?,
            }),
            "text" => Ok(Self::Text {
                selector: get_str("selector")?,
            }),
            "count" => Ok(Self::Count {
                selector: get_str("selector")?,
            }),
            "attr" => Ok(Self::Attr {
                selector: get_str("selector")?,
                name: get_str("name")?,
            }),
            "select-text" => Ok(Self::SelectText {
                selector: get_str("selector")?,
                substring: get_str("substring")?,
            }),
            "drop" => {
                let paths = map
                    .get("paths")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "drop requires array field 'paths'".to_string())?
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(ToOwned::to_owned)
                            .ok_or_else(|| "drop paths must be strings".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if paths.is_empty() {
                    return Err("drop requires at least one path".to_string());
                }
                Ok(Self::Drop { paths })
            }
            "snapshot" => Ok(Self::Snapshot {
                path: get_str("path").or_else(|_| get_str("file"))?,
            }),
            "eval" => Ok(Self::Eval { js: get_str("js")? }),
            "resize" => {
                let width = map
                    .get("width")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "resize requires width".to_string())?
                    as u32;
                let height = map
                    .get("height")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "resize requires height".to_string())?
                    as u32;
                Ok(Self::Resize { width, height })
            }
            "appearance" => Ok(Self::Appearance {
                mode: appearance_mode(&get_str("mode")?)?,
            }),
            other => Err(format!("unknown command: {other}")),
        }
    }

    pub fn to_request(&self, id: Value) -> Request {
        match self {
            Self::WaitFor {
                selector,
                timeout_ms,
            } => Request {
                id,
                cmd: "wait-for".into(),
                args: json!([selector, "--timeout", timeout_ms.to_string()]),
            },
            Self::Click { selector } => Request {
                id,
                cmd: "click".into(),
                args: json!([selector]),
            },
            Self::Type { selector, text } => Request {
                id,
                cmd: "type".into(),
                args: json!([selector, text]),
            },
            Self::Press { key } => Request {
                id,
                cmd: "press".into(),
                args: json!([key]),
            },
            Self::Text { selector } => Request {
                id,
                cmd: "text".into(),
                args: json!([selector]),
            },
            Self::Count { selector } => Request {
                id,
                cmd: "count".into(),
                args: json!([selector]),
            },
            Self::Attr { selector, name } => Request {
                id,
                cmd: "attr".into(),
                args: json!([selector, name]),
            },
            Self::SelectText {
                selector,
                substring,
            } => Request {
                id,
                cmd: "select-text".into(),
                args: json!([selector, substring]),
            },
            Self::Drop { paths } => Request {
                id,
                cmd: "drop".into(),
                args: json!(paths),
            },
            Self::Snapshot { path } => Request {
                id,
                cmd: "snapshot".into(),
                args: json!([path]),
            },
            Self::Eval { js } => Request {
                id,
                cmd: "eval".into(),
                args: json!([js]),
            },
            Self::Resize { width, height } => Request {
                id,
                cmd: "resize".into(),
                args: json!([width.to_string(), height.to_string()]),
            },
            Self::Appearance { mode } => Request {
                id,
                cmd: "appearance".into(),
                args: json!([mode]),
            },
        }
    }
}

/// Resolve the control socket path from `$TUTORDOCS_CTL_SOCKET` or `<temp_dir>/tutordocs-ctl/<pid>.sock`.
pub fn resolve_socket_path(env_socket: Option<&OsStr>, temp_dir: &Path, pid: u32) -> PathBuf {
    if let Some(raw) = env_socket {
        if !raw.is_empty() {
            return PathBuf::from(raw);
        }
    }
    temp_dir.join("tutordocs-ctl").join(format!("{pid}.sock"))
}

/// Ensure the parent directory exists with mode 0700 and any stale socket is removed.
pub fn prepare_socket_dir(socket_path: &Path) -> Result<(), String> {
    if let Some(parent) = socket_path.parent() {
        if !parent.as_os_str().is_empty() {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder
                .create(parent)
                .map_err(|e| format!("failed to create socket dir {}: {e}", parent.display()))?;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("failed to chmod 0700 {}: {e}", parent.display()))?;
        }
    }
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }
    Ok(())
}

pub trait CtlBackend: Send + Sync + 'static {
    fn execute(&self, cmd: CtlCommand) -> Result<Value, String>;
}

pub struct CtlServer {
    socket_path: PathBuf,
    shutdown: Arc<AtomicBool>,
}

impl CtlServer {
    pub fn bind(socket_path: PathBuf, backend: Arc<dyn CtlBackend>) -> Result<Self, String> {
        prepare_socket_dir(&socket_path)?;
        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| format!("failed to bind socket {}: {e}", socket_path.display()))?;
        let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600));

        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = shutdown.clone();
        std::thread::Builder::new()
            .name("tutordocs-ctl-server".into())
            .spawn(move || {
                for incoming in listener.incoming() {
                    if flag.load(Ordering::SeqCst) {
                        break;
                    }
                    match incoming {
                        Ok(stream) => {
                            let backend = backend.clone();
                            std::thread::spawn(move || handle_stream(stream, backend));
                        }
                        Err(_) => {
                            if flag.load(Ordering::SeqCst) {
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|e| format!("failed to spawn control socket thread: {e}"))?;

        Ok(Self {
            socket_path,
            shutdown,
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for CtlServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.socket_path);
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

fn handle_stream(stream: UnixStream, backend: Arc<dyn CtlBackend>) {
    let Ok(writer_stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut writer = writer_stream;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let reply = match serde_json::from_str::<Request>(trimmed) {
                    Ok(req) => match CtlCommand::from_request(&req) {
                        Ok(cmd) => match backend.execute(cmd) {
                            Ok(val) => Reply::ok(req.id, val),
                            Err(err) => Reply::err(req.id, err),
                        },
                        Err(err) => Reply::err(req.id, err),
                    },
                    Err(err) => Reply::err(Value::Null, format!("invalid JSON request: {err}")),
                };
                if let Ok(mut encoded) = serde_json::to_string(&reply) {
                    encoded.push('\n');
                    let _ = writer.write_all(encoded.as_bytes());
                    let _ = writer.flush();
                }
            }
            Err(_) => break,
        }
    }
}

/// Send one request over the Unix socket and read one reply line.
pub fn send_request(socket_path: &Path, req: &Request) -> Result<Reply, String> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("could not connect to {}: {e}", socket_path.display()))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let mut payload =
        serde_json::to_string(req).map_err(|e| format!("failed to serialize request: {e}"))?;
    payload.push('\n');
    stream
        .write_all(payload.as_bytes())
        .map_err(|e| format!("failed to write request: {e}"))?;
    stream
        .flush()
        .map_err(|e| format!("failed to flush request: {e}"))?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .map_err(|e| format!("failed to read reply: {e}"))?;
    if n == 0 {
        return Err("connection closed before reply".to_string());
    }
    serde_json::from_str::<Reply>(line.trim())
        .map_err(|e| format!("invalid reply JSON '{line}': {e}"))
}

/// Build the JavaScript snippet that executes a page command and invokes `__ctl_result`.
pub fn js_for_page_command(call_id: &str, cmd: &CtlCommand) -> Result<String, String> {
    let mut eval_source = "null".to_string();
    let (cmd_name, payload) = match cmd {
        CtlCommand::WaitFor {
            selector,
            timeout_ms,
        } => (
            "wait-for",
            json!({ "selector": selector, "timeoutMs": timeout_ms }),
        ),
        CtlCommand::Click { selector } => ("click", json!({ "selector": selector })),
        CtlCommand::Type { selector, text } => {
            ("type", json!({ "selector": selector, "text": text }))
        }
        CtlCommand::Press { key } => ("press", json!({ "key": key })),
        CtlCommand::Text { selector } => ("text", json!({ "selector": selector })),
        CtlCommand::Count { selector } => ("count", json!({ "selector": selector })),
        CtlCommand::Attr { selector, name } => {
            ("attr", json!({ "selector": selector, "name": name }))
        }
        CtlCommand::SelectText {
            selector,
            substring,
        } => (
            "select-text",
            json!({ "selector": selector, "substring": substring }),
        ),
        CtlCommand::Eval { js } => {
            eval_source = js.clone();
            ("eval", json!({ "js": js }))
        }
        CtlCommand::Drop { .. }
        | CtlCommand::Snapshot { .. }
        | CtlCommand::Resize { .. }
        | CtlCommand::Appearance { .. } => {
            return Err("command is handled on the native side, not in page JS".to_string())
        }
    };

    let call_id_json = serde_json::to_string(call_id).unwrap();
    let cmd_name_json = serde_json::to_string(cmd_name).unwrap();
    let payload_json = serde_json::to_string(&payload).unwrap();

    let script = format!(
        r#"(function() {{
  const callId = {call_id_json};
  const cmdName = {cmd_name_json};
  const payload = {payload_json};
  const sendResult = async (ok, value, error) => {{
    for (let i = 0; i < 100 && (!window.__TAURI_INTERNALS__ || typeof window.__TAURI_INTERNALS__.invoke !== "function"); i++) {{
      await new Promise((r) => setTimeout(r, 20));
    }}
    await window.__TAURI_INTERNALS__.invoke("__ctl_result", {{
      id: callId,
      ok,
      value: ok ? (value === undefined ? null : value) : null,
      error: ok ? null : String(error),
    }});
  }};
  const isVisible = (el, selector) => {{
    if (!(el instanceof Element)) return false;
    if (String(selector).includes("hidden")) return true;
    return el.closest("[hidden]") === null;
  }};
  const findOne = (selector) => {{
    const all = Array.from(document.querySelectorAll(selector));
    return all.find((el) => isVisible(el, selector)) || all[0] || null;
  }};
  const requireVisible = (selector) => {{
    const all = Array.from(document.querySelectorAll(selector));
    const found = all.find((el) => isVisible(el, selector));
    if (!found) throw new Error("no visible element matches selector: " + selector);
    return found;
  }};
  (async () => {{
    switch (cmdName) {{
      case "wait-for": {{
        const deadline = Date.now() + Number(payload.timeoutMs || 250);
        while (true) {{
          const all = Array.from(document.querySelectorAll(payload.selector));
          const found = all.find((el) => isVisible(el, payload.selector));
          if (found) return (found.textContent || "").trim();
          if (Date.now() >= deadline) throw new Error("timed out waiting for selector: " + payload.selector);
          await new Promise((r) => setTimeout(r, 25));
        }}
      }}
      case "click": {{
        const el = requireVisible(payload.selector);
        if (el instanceof HTMLElement && typeof el.scrollIntoView === "function") {{
          el.scrollIntoView({{ block: "nearest", inline: "nearest" }});
        }}
        const md = new MouseEvent("mousedown", {{ bubbles: true, cancelable: true, view: window }});
        const notPrevented = el.dispatchEvent(md);
        if (notPrevented && el instanceof HTMLElement && typeof el.focus === "function") {{
          el.focus();
        }}
        if (el instanceof HTMLElement && typeof el.click === "function") {{
          el.click();
        }} else {{
          el.dispatchEvent(new MouseEvent("click", {{ bubbles: true, cancelable: true, view: window }}));
        }}
        el.dispatchEvent(new MouseEvent("mouseup", {{ bubbles: true, cancelable: true, view: window }}));
        return true;
      }}
      case "type": {{
        const el = requireVisible(payload.selector);
        if (el instanceof HTMLElement && typeof el.focus === "function") {{
          el.focus();
        }}
        if ("value" in el) {{
          el.value = payload.text;
        }} else {{
          el.textContent = payload.text;
        }}
        el.dispatchEvent(new InputEvent("input", {{ bubbles: true, cancelable: false, data: payload.text, inputType: "insertText" }}));
        el.dispatchEvent(new Event("change", {{ bubbles: true }}));
        if (el.classList && el.classList.contains("note-field")) {{
          el.dispatchEvent(new FocusEvent("blur", {{ bubbles: false }}));
        }}
        return payload.text;
      }}
      case "press": {{
        const key = payload.key;
        const target = (document.activeElement && document.activeElement !== document.body)
          ? document.activeElement
          : (document.body || document.documentElement);
        const kd = new KeyboardEvent("keydown", {{ key, bubbles: true, cancelable: true }});
        const notPrevented = target.dispatchEvent(kd);
        if (notPrevented && key === "Enter" && target instanceof HTMLElement) {{
          const form = target.closest("form");
          if (form && typeof form.requestSubmit === "function") {{
            form.requestSubmit();
          }}
        }}
        target.dispatchEvent(new KeyboardEvent("keyup", {{ key, bubbles: true, cancelable: true }}));
        return key;
      }}
      case "text": {{
        const el = findOne(payload.selector);
        if (!el) throw new Error("no element matches selector: " + payload.selector);
        if ((el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) && (el.textContent || "") === "") {{
          return el.value;
        }}
        return el.textContent || "";
      }}
      case "count": {{
        const all = Array.from(document.querySelectorAll(payload.selector));
        return all.filter((el) => isVisible(el, payload.selector)).length;
      }}
      case "attr": {{
        const el = findOne(payload.selector);
        if (!el) throw new Error("no element matches selector: " + payload.selector);
        const name = payload.name;
        if (name === "value" && "value" in el) return el.value;
        if (name === "checked" && "checked" in el) return String(Boolean(el.checked));
        if (name === "disabled" && "disabled" in el) return el.disabled ? "true" : null;
        return el.getAttribute(name);
      }}
      case "select-text": {{
        const el = requireVisible(payload.selector);
        const sub = String(payload.substring);
        if (!sub) throw new Error("substring must not be empty");
        const textNodes = [];
        const walk = (node) => {{
          for (const child of Array.from(node.childNodes)) {{
            if (child.nodeType === Node.TEXT_NODE) {{
              textNodes.push(child);
            }} else if (child instanceof Element && !child.classList.contains("katex-mathml")) {{
              walk(child);
            }}
          }}
        }};
        walk(el);
        let full = "";
        const spans = [];
        for (const tn of textNodes) {{
          const start = full.length;
          full += tn.data;
          spans.push({{ node: tn, start, end: full.length }});
        }}
        const idx = full.indexOf(sub);
        if (idx === -1) throw new Error("substring not found in " + payload.selector + ": " + sub);
        const endIdx = idx + sub.length;
        const startSpan = spans.find((s) => idx >= s.start && idx < s.end);
        const endSpan = spans.find((s) => endIdx > s.start && endIdx <= s.end);
        if (!startSpan || !endSpan) throw new Error("could not map substring range in " + payload.selector);
        const range = document.createRange();
        range.setStart(startSpan.node, idx - startSpan.start);
        range.setEnd(endSpan.node, endIdx - endSpan.start);
        const sel = document.getSelection();
        if (!sel) throw new Error("document.getSelection() returned null");
        sel.removeAllRanges();
        sel.addRange(range);
        document.dispatchEvent(new Event("selectionchange"));
        el.dispatchEvent(new MouseEvent("mouseup", {{ bubbles: true, cancelable: true, view: window }}));
        return sel.toString();
      }}
      case "eval": {{
        // Inlined into this script. new Function is blocked by the page CSP (script-src 'self').
        const res = await (async () => (__CTL_EVAL_SOURCE__))();
        return res === undefined ? null : res;
      }}
      default:
        throw new Error("unsupported page command: " + cmdName);
    }}
  }})().then(
    (val) => sendResult(true, val, null),
    (err) => sendResult(false, null, err && err.message ? err.message : String(err))
  );
}})();"#
    );
    Ok(script.replace("__CTL_EVAL_SOURCE__", &eval_source))
}

/// State registered on the Tauri app only when `e2e-control` is enabled.
#[cfg(feature = "e2e-control")]
#[derive(Default)]
pub struct CtlState {
    pending: Mutex<HashMap<String, mpsc::Sender<Result<Value, String>>>>,
    next_id: AtomicU64,
    server: Mutex<Option<CtlServer>>,
}

#[cfg(feature = "e2e-control")]
impl CtlState {
    pub fn register_call(&self) -> (String, mpsc::Receiver<Result<Value, String>>) {
        let id = format!("ctl-{}", self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);
        (id, rx)
    }

    pub fn resolve_call(&self, id: &str, result: Result<Value, String>) -> Result<(), String> {
        let tx = self
            .pending
            .lock()
            .unwrap()
            .remove(id)
            .ok_or_else(|| format!("unknown or expired control call id: {id}"))?;
        let _ = tx.send(result);
        Ok(())
    }

    pub fn cancel_call(&self, id: &str) {
        self.pending.lock().unwrap().remove(id);
    }
}

/// Tauri IPC command that receives the result of a page command. Exists only under `e2e-control`.
#[cfg(feature = "e2e-control")]
#[tauri::command]
pub fn __ctl_result(
    state: tauri::State<'_, CtlState>,
    id: String,
    ok: bool,
    value: Option<Value>,
    error: Option<String>,
) -> Result<(), String> {
    let outcome = if ok {
        Ok(value.unwrap_or(Value::Null))
    } else {
        Err(error.unwrap_or_else(|| "page command failed".to_string()))
    };
    state.resolve_call(&id, outcome)
}

/// Emit the drag-drop event that `getCurrentWebview().onDragDropEvent` listens for.
pub fn emit_drop_on_app<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    paths: &[String],
) -> Result<Value, String> {
    use tauri::Emitter;
    app.emit(
        "tauri://drag-drop",
        json!({
            "paths": paths,
            "position": { "x": 0.0, "y": 0.0 }
        }),
    )
    .map_err(|e| format!("failed to emit drag-drop event: {e}"))?;
    Ok(json!({ "dropped": paths.len(), "paths": paths }))
}

/// Ask the web view what it is showing. Used when a page command never calls back,
/// so a timeout says whether the page is loaded or the script never ran.
#[cfg(feature = "e2e-control")]
fn probe_page<R: tauri::Runtime>(webview: &tauri::WebviewWindow<R>) -> String {
    let (tx, rx) = mpsc::channel();
    let visible = webview.is_visible().ok();
    if let Err(err) = webview.eval_with_callback(
        "document.readyState + ' | ' + document.title + ' | ' + (document.body ? document.body.innerText.slice(0, 160) : 'no-body')",
        move |result| {
            let _ = tx.send(result);
        },
    ) {
        return format!("page probe failed to start: {err}; visible={visible:?}");
    }
    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(result) => format!("page probe: {result}; visible={visible:?}"),
        Err(_) => format!("page probe got no callback; visible={visible:?}"),
    }
}

#[cfg(feature = "e2e-control")]
struct TauriCtlBackend<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

#[cfg(feature = "e2e-control")]
impl<R: tauri::Runtime> TauriCtlBackend<R> {
    fn webview(&self) -> Result<tauri::WebviewWindow<R>, String> {
        use tauri::Manager;
        if let Some(w) = self.app.get_webview_window("reader") {
            return Ok(w);
        }
        self.app
            .webview_windows()
            .into_values()
            .next()
            .ok_or_else(|| "no webview window available".to_string())
    }

    fn run_page_once(&self, cmd: &CtlCommand, wait_timeout: Duration) -> Result<Value, String> {
        use tauri::Manager;
        let webview = self.webview()?;
        let state = self.app.state::<CtlState>();
        let (call_id, rx) = state.register_call();
        let script = match js_for_page_command(&call_id, cmd) {
            Ok(s) => s,
            Err(e) => {
                state.cancel_call(&call_id);
                return Err(e);
            }
        };
        if let Err(e) = webview.eval(&script) {
            state.cancel_call(&call_id);
            return Err(format!("webview eval failed: {e}"));
        }
        match rx.recv_timeout(wait_timeout) {
            Ok(result) => result,
            Err(_) => {
                state.cancel_call(&call_id);
                let probe = probe_page(&webview);
                Err(format!(
                    "timed out waiting for page reply on {call_id}; {probe}"
                ))
            }
        }
    }
}

#[cfg(feature = "e2e-control")]
impl<R: tauri::Runtime> CtlBackend for TauriCtlBackend<R> {
    fn execute(&self, cmd: CtlCommand) -> Result<Value, String> {
        match cmd {
            CtlCommand::Drop { paths } => emit_drop_on_app(&self.app, &paths),
            CtlCommand::Snapshot { path } => {
                take_webview_snapshot(&self.webview()?, Path::new(&path))
            }
            CtlCommand::Resize { width, height } => {
                let window = self.webview()?;
                window
                    .set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                        width as f64,
                        height as f64,
                    )))
                    .map_err(|e| format!("failed to resize window: {e}"))?;
                Ok(json!({ "width": width, "height": height }))
            }
            CtlCommand::Appearance { mode } => {
                let theme = match mode.as_str() {
                    "light" => Some(tauri::Theme::Light),
                    "dark" => Some(tauri::Theme::Dark),
                    _ => None,
                };
                self.webview()?
                    .set_theme(theme)
                    .map_err(|e| format!("failed to set the window appearance: {e}"))?;
                Ok(json!({ "appearance": mode }))
            }
            CtlCommand::WaitFor {
                selector,
                timeout_ms,
            } => {
                let deadline = Instant::now() + Duration::from_millis(timeout_ms);
                let mut last_err = format!("timed out waiting for selector: {selector}");
                loop {
                    let now = Instant::now();
                    if now >= deadline {
                        return Err(last_err);
                    }
                    let remaining_ms = deadline.duration_since(now).as_millis() as u64;
                    let slice_ms = remaining_ms.clamp(50, WAIT_SLICE_MS);
                    let slice_cmd = CtlCommand::WaitFor {
                        selector: selector.clone(),
                        timeout_ms: slice_ms,
                    };
                    match self.run_page_once(&slice_cmd, Duration::from_millis(slice_ms + 500)) {
                        Ok(val) => return Ok(val),
                        Err(err) => {
                            last_err = err;
                            if Instant::now() >= deadline {
                                return Err(last_err);
                            }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                    }
                }
            }
            other => self.run_page_once(&other, Duration::from_millis(DEFAULT_CMD_TIMEOUT_MS)),
        }
    }
}

#[cfg(all(feature = "e2e-control", target_os = "macos"))]
fn take_webview_snapshot<R: tauri::Runtime>(
    webview: &tauri::WebviewWindow<R>,
    output_path: &Path,
) -> Result<Value, String> {
    use block2::RcBlock;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use objc2_web_kit::WKWebView;

    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    webview
        .with_webview(move |platform| {
            let ptr = platform.inner() as *const WKWebView;
            if ptr.is_null() {
                let _ = tx.send(Err("WKWebView pointer is null".to_string()));
                return;
            }
            let wk: &WKWebView = unsafe { &*ptr };
            let tx_inner = tx.clone();
            let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                let res = (|| -> Result<Vec<u8>, String> {
                    if !error.is_null() {
                        let err = unsafe { &*error };
                        return Err(format!(
                            "WKWebView takeSnapshot error: {}",
                            err.localizedDescription()
                        ));
                    }
                    if image.is_null() {
                        return Err("WKWebView takeSnapshot returned null image".to_string());
                    }
                    let ns_image = unsafe { &*image };
                    let tiff = ns_image
                        .TIFFRepresentation()
                        .ok_or_else(|| "NSImage TIFFRepresentation returned nil".to_string())?;
                    let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or_else(|| {
                        "NSBitmapImageRep imageRepWithData returned nil".to_string()
                    })?;
                    let props = NSDictionary::new();
                    let png = unsafe {
                        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props)
                    }
                    .ok_or_else(|| {
                        "NSBitmapImageRep PNG representation returned nil".to_string()
                    })?;
                    Ok(png.to_vec())
                })();
                let _ = tx_inner.send(res);
            });
            unsafe {
                wk.takeSnapshotWithConfiguration_completionHandler(None, &block);
            }
        })
        .map_err(|e| format!("with_webview failed: {e}"))?;

    let png_bytes = rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "timed out waiting for WKWebView takeSnapshot".to_string())??;

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create snapshot dir {}: {e}", parent.display()))?;
        }
    }
    std::fs::write(output_path, &png_bytes)
        .map_err(|e| format!("failed to write snapshot {}: {e}", output_path.display()))?;
    Ok(json!({
        "path": output_path.to_string_lossy(),
        "bytes": png_bytes.len()
    }))
}

#[cfg(all(feature = "e2e-control", not(target_os = "macos")))]
fn take_webview_snapshot<R: tauri::Runtime>(
    _webview: &tauri::WebviewWindow<R>,
    _output_path: &Path,
) -> Result<Value, String> {
    Err("snapshot is only supported on macOS WKWebView".to_string())
}

#[cfg(feature = "e2e-control")]
static SIGNAL_SOCKET_PATH: Mutex<Option<CString>> = Mutex::new(None);

#[cfg(feature = "e2e-control")]
extern "C" {
    fn unlink(path: *const std::ffi::c_char) -> i32;
    fn signal(sig: i32, handler: usize) -> usize;
    fn _exit(status: i32) -> !;
}

#[cfg(feature = "e2e-control")]
extern "C" fn handle_termination_signal(_sig: i32) {
    if let Ok(guard) = SIGNAL_SOCKET_PATH.try_lock() {
        if let Some(ref cpath) = *guard {
            unsafe {
                unlink(cpath.as_ptr());
            }
        }
    }
    unsafe {
        _exit(0);
    }
}

#[cfg(feature = "e2e-control")]
fn install_signal_cleanup(socket_path: &Path) {
    if let Ok(cstr) = CString::new(socket_path.as_os_str().to_string_lossy().as_bytes()) {
        if let Ok(mut guard) = SIGNAL_SOCKET_PATH.lock() {
            *guard = Some(cstr);
        }
        unsafe {
            const SIGINT: i32 = 2;
            const SIGTERM: i32 = 15;
            signal(SIGINT, handle_termination_signal as *const () as usize);
            signal(SIGTERM, handle_termination_signal as *const () as usize);
        }
    }
}

/// Start the control socket server for a running Tauri application (only under `e2e-control`).
#[cfg(feature = "e2e-control")]
pub fn start_for_app<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    let sock_path = resolve_socket_path(
        std::env::var_os("TUTORDOCS_CTL_SOCKET").as_deref(),
        &std::env::temp_dir(),
        std::process::id(),
    );
    install_signal_cleanup(&sock_path);
    let backend = Arc::new(TauriCtlBackend { app: app.clone() });
    let server = CtlServer::bind(sock_path.clone(), backend)?;
    let state = app.state::<CtlState>();
    *state.server.lock().unwrap() = Some(server);
    eprintln!("tutordocs-ctl socket {}", sock_path.display());
    Ok(sock_path)
}

#[cfg(feature = "e2e-control")]
pub fn cleanup_for_app<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    if let Some(state) = app.try_state::<CtlState>() {
        let _ = state.server.lock().unwrap().take();
    }
}

fn discover_socket_path(
    explicit_socket: Option<&Path>,
    explicit_pid: Option<u32>,
) -> Result<PathBuf, String> {
    if let Some(path) = explicit_socket {
        return Ok(path.to_path_buf());
    }
    if let Some(pid) = explicit_pid {
        return Ok(resolve_socket_path(None, &std::env::temp_dir(), pid));
    }
    if let Some(env_sock) = std::env::var_os("TUTORDOCS_CTL_SOCKET") {
        if !env_sock.is_empty() {
            return Ok(PathBuf::from(env_sock));
        }
    }
    let dir = std::env::temp_dir().join("tutordocs-ctl");
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut socks: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("sock"))
            .collect();
        if socks.len() == 1 {
            return Ok(socks.remove(0));
        }
    }
    Err("set TUTORDOCS_CTL_SOCKET or pass --socket <path> / --pid <pid>".to_string())
}

/// Entrypoint for the `tutordocs-ctl` CLI binary.
pub fn run_cli(args: Vec<String>) -> i32 {
    let mut socket_override: Option<PathBuf> = None;
    let mut pid_override: Option<u32> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--socket" {
            let Some(val) = args.get(i + 1) else {
                eprintln!("tutordocs-ctl: --socket requires a path");
                return 2;
            };
            socket_override = Some(PathBuf::from(val));
            i += 2;
        } else if args[i] == "--pid" {
            let Some(val) = args.get(i + 1) else {
                eprintln!("tutordocs-ctl: --pid requires a PID");
                return 2;
            };
            match val.parse::<u32>() {
                Ok(pid) => pid_override = Some(pid),
                Err(_) => {
                    eprintln!("tutordocs-ctl: invalid --pid: {val}");
                    return 2;
                }
            }
            i += 2;
        } else {
            break;
        }
    }

    let cmd = match CtlCommand::from_cli_args(&args[i..]) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("tutordocs-ctl: {err}");
            return 2;
        }
    };
    let sock_path = match discover_socket_path(socket_override.as_deref(), pid_override) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("tutordocs-ctl: {err}");
            return 1;
        }
    };
    let req = cmd.to_request(json!(1));
    match send_request(&sock_path, &req) {
        Ok(Reply {
            result: Ok(val), ..
        }) => {
            match val {
                Value::Null => {}
                Value::String(s) => println!("{s}"),
                Value::Number(n) => println!("{n}"),
                Value::Bool(b) => println!("{b}"),
                other => println!("{other}"),
            }
            0
        }
        Ok(Reply {
            result: Err(err), ..
        }) => {
            eprintln!("tutordocs-ctl: {err}");
            1
        }
        Err(err) => {
            eprintln!("tutordocs-ctl: {err}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tauri::Listener;
    #[cfg(feature = "e2e-control")]
    use tauri::Manager;

    #[cfg(not(feature = "e2e-control"))]
    #[test]
    fn default_build_has_no_control_socket_and_no_ctl_result_command() {
        assert!(!is_enabled());
        let dir = tempfile::tempdir().unwrap();
        let app =
            crate::reader::install(mock_builder(), crate::reader::ReaderState::open(dir.path()))
                .build(mock_context(noop_assets()))
                .expect("mock app builds");
        let webview = tauri::WebviewWindowBuilder::new(&app, "reader", Default::default())
            .build()
            .expect("mock webview builds");

        let res = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "__ctl_result".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(json!({ "id": "1", "ok": true, "value": null })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        );
        assert!(res.is_err(), "default build must not expose __ctl_result");
        let socket_dir = std::env::temp_dir().join("tutordocs-ctl");
        assert!(
            !socket_dir
                .join(format!("{}.sock", std::process::id()))
                .exists(),
            "default build must not open a control socket"
        );
    }

    #[cfg(feature = "e2e-control")]
    #[test]
    fn feature_build_exposes_ctl_result_command_and_resolves_pending_call() {
        assert!(is_enabled());
        let dir = tempfile::tempdir().unwrap();
        let app =
            crate::reader::install(mock_builder(), crate::reader::ReaderState::open(dir.path()))
                .build(mock_context(noop_assets()))
                .expect("mock app builds");
        let webview = tauri::WebviewWindowBuilder::new(&app, "reader", Default::default())
            .build()
            .expect("mock webview builds");

        let state = app.state::<CtlState>();
        let (call_id, rx) = state.register_call();
        let res = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "__ctl_result".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(json!({
                    "id": call_id,
                    "ok": true,
                    "value": "Cell Membranes",
                    "error": null
                })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        );
        assert!(res.is_ok(), "e2e-control build must expose __ctl_result");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Ok(json!("Cell Membranes"))
        );
    }

    #[test]
    fn socket_path_uses_env_or_temp_dir_pid() {
        let tmp = Path::new("/tmp/tutordocs-test-tmp");
        assert_eq!(
            resolve_socket_path(Some(OsStr::new("/custom/ctl.sock")), tmp, 1234),
            PathBuf::from("/custom/ctl.sock")
        );
        assert_eq!(
            resolve_socket_path(None, tmp, 4321),
            PathBuf::from("/tmp/tutordocs-test-tmp/tutordocs-ctl/4321.sock")
        );
        assert_eq!(
            resolve_socket_path(Some(OsStr::new("")), tmp, 4321),
            PathBuf::from("/tmp/tutordocs-test-tmp/tutordocs-ctl/4321.sock")
        );
    }

    #[test]
    fn parses_every_command_from_cli_and_wire_json() {
        let cases: Vec<(Vec<&str>, CtlCommand)> = vec![
            (
                vec!["wait-for", ".home-new"],
                CtlCommand::WaitFor {
                    selector: ".home-new".into(),
                    timeout_ms: DEFAULT_WAIT_TIMEOUT_MS,
                },
            ),
            (
                vec!["wait-for", ".home-new", "--timeout", "2500"],
                CtlCommand::WaitFor {
                    selector: ".home-new".into(),
                    timeout_ms: 2500,
                },
            ),
            (
                vec!["click", "button.submit"],
                CtlCommand::Click {
                    selector: "button.submit".into(),
                },
            ),
            (
                vec!["type", "input.title", "Cell Membranes"],
                CtlCommand::Type {
                    selector: "input.title".into(),
                    text: "Cell Membranes".into(),
                },
            ),
            (
                vec!["press", "ArrowUp"],
                CtlCommand::Press {
                    key: "ArrowUp".into(),
                },
            ),
            (
                vec!["text", "h1"],
                CtlCommand::Text {
                    selector: "h1".into(),
                },
            ),
            (
                vec!["count", "li.draft-chapter"],
                CtlCommand::Count {
                    selector: "li.draft-chapter".into(),
                },
            ),
            (
                vec!["attr", ".review-status", "data-status"],
                CtlCommand::Attr {
                    selector: ".review-status".into(),
                    name: "data-status".into(),
                },
            ),
            (
                vec!["select-text", ".prose", "selective barrier"],
                CtlCommand::SelectText {
                    selector: ".prose".into(),
                    substring: "selective barrier".into(),
                },
            ),
            (
                vec!["drop", "/tmp/a.pdf", "/tmp/b.md"],
                CtlCommand::Drop {
                    paths: vec!["/tmp/a.pdf".into(), "/tmp/b.md".into()],
                },
            ),
            (
                vec!["snapshot", "/tmp/shot.png"],
                CtlCommand::Snapshot {
                    path: "/tmp/shot.png".into(),
                },
            ),
            (
                vec!["eval", "1 + 1"],
                CtlCommand::Eval { js: "1 + 1".into() },
            ),
        ];

        for (cli, expected) in cases {
            let owned: Vec<String> = cli.iter().map(|s| (*s).to_string()).collect();
            let parsed = CtlCommand::from_cli_args(&owned).expect("CLI args parse");
            assert_eq!(parsed, expected);

            let req = parsed.to_request(json!(1));
            let roundtrip = CtlCommand::from_request(&req).expect("wire request parses");
            assert_eq!(roundtrip, expected);
        }

        // Also test object-shaped wire args for every command.
        let obj_cases: Vec<(Request, CtlCommand)> = vec![
            (
                Request {
                    id: json!(1),
                    cmd: "wait-for".into(),
                    args: json!({ "selector": ".ready", "timeout": 1200 }),
                },
                CtlCommand::WaitFor {
                    selector: ".ready".into(),
                    timeout_ms: 1200,
                },
            ),
            (
                Request {
                    id: json!(2),
                    cmd: "click".into(),
                    args: json!({ "selector": "button" }),
                },
                CtlCommand::Click {
                    selector: "button".into(),
                },
            ),
            (
                Request {
                    id: json!(3),
                    cmd: "type".into(),
                    args: json!({ "selector": "input", "text": "hello" }),
                },
                CtlCommand::Type {
                    selector: "input".into(),
                    text: "hello".into(),
                },
            ),
            (
                Request {
                    id: json!(4),
                    cmd: "press".into(),
                    args: json!({ "key": "Enter" }),
                },
                CtlCommand::Press {
                    key: "Enter".into(),
                },
            ),
            (
                Request {
                    id: json!(5),
                    cmd: "text".into(),
                    args: json!({ "selector": "p" }),
                },
                CtlCommand::Text {
                    selector: "p".into(),
                },
            ),
            (
                Request {
                    id: json!(6),
                    cmd: "count".into(),
                    args: json!({ "selector": "li" }),
                },
                CtlCommand::Count {
                    selector: "li".into(),
                },
            ),
            (
                Request {
                    id: json!(7),
                    cmd: "attr".into(),
                    args: json!({ "selector": "a", "name": "href" }),
                },
                CtlCommand::Attr {
                    selector: "a".into(),
                    name: "href".into(),
                },
            ),
            (
                Request {
                    id: json!(8),
                    cmd: "select-text".into(),
                    args: json!({ "selector": ".prose", "substring": "quote" }),
                },
                CtlCommand::SelectText {
                    selector: ".prose".into(),
                    substring: "quote".into(),
                },
            ),
            (
                Request {
                    id: json!(9),
                    cmd: "drop".into(),
                    args: json!({ "paths": ["/a.pdf"] }),
                },
                CtlCommand::Drop {
                    paths: vec!["/a.pdf".into()],
                },
            ),
            (
                Request {
                    id: json!(10),
                    cmd: "snapshot".into(),
                    args: json!({ "path": "/out.png" }),
                },
                CtlCommand::Snapshot {
                    path: "/out.png".into(),
                },
            ),
            (
                Request {
                    id: json!(11),
                    cmd: "eval".into(),
                    args: json!({ "js": "document.title" }),
                },
                CtlCommand::Eval {
                    js: "document.title".into(),
                },
            ),
            (
                Request {
                    id: json!(12),
                    cmd: "resize".into(),
                    args: json!({ "width": 900, "height": 600 }),
                },
                CtlCommand::Resize {
                    width: 900,
                    height: 600,
                },
            ),
            (
                Request {
                    id: json!(13),
                    cmd: "appearance".into(),
                    args: json!({ "mode": "dark" }),
                },
                CtlCommand::Appearance {
                    mode: "dark".into(),
                },
            ),
        ];
        for (req, expected) in obj_cases {
            assert_eq!(CtlCommand::from_request(&req).unwrap(), expected);
        }

        // Page commands build valid JS snippets invoking __ctl_result, while native commands reject js_for_page_command.
        for cmd in [
            CtlCommand::WaitFor {
                selector: ".a".into(),
                timeout_ms: 100,
            },
            CtlCommand::Click {
                selector: ".a".into(),
            },
            CtlCommand::Type {
                selector: ".a".into(),
                text: "b".into(),
            },
            CtlCommand::Press {
                key: "Escape".into(),
            },
            CtlCommand::Text {
                selector: ".a".into(),
            },
            CtlCommand::Count {
                selector: ".a".into(),
            },
            CtlCommand::Attr {
                selector: ".a".into(),
                name: "id".into(),
            },
            CtlCommand::SelectText {
                selector: ".a".into(),
                substring: "hi".into(),
            },
            CtlCommand::Eval { js: "2 + 2".into() },
        ] {
            let js = js_for_page_command("call-42", &cmd).expect("page command builds JS");
            assert!(js.contains("__ctl_result"));
            assert!(js.contains("call-42"));
        }
        assert!(js_for_page_command(
            "call-43",
            &CtlCommand::Drop {
                paths: vec!["/x".into()]
            }
        )
        .is_err());
        assert!(js_for_page_command(
            "call-44",
            &CtlCommand::Snapshot {
                path: "/x.png".into()
            }
        )
        .is_err());
        assert!(js_for_page_command(
            "call-45",
            &CtlCommand::Resize {
                width: 900,
                height: 600,
            }
        )
        .is_err());
        assert!(js_for_page_command(
            "call-46",
            &CtlCommand::Appearance {
                mode: "light".into()
            }
        )
        .is_err());
        assert_eq!(
            CtlCommand::from_cli_args(&["appearance".into(), "system".into()]).unwrap(),
            CtlCommand::Appearance {
                mode: "system".into()
            }
        );
        assert!(CtlCommand::from_cli_args(&["appearance".into(), "sepia".into()]).is_err());

        assert!(CtlCommand::from_cli_args(&[]).is_err());
        assert!(CtlCommand::from_request(&Request {
            id: json!(1),
            cmd: "unknown-cmd".into(),
            args: json!([]),
        })
        .is_err());
    }

    #[test]
    fn drop_command_emits_tauri_drag_drop_event_with_paths() {
        let dir = tempfile::tempdir().unwrap();
        let app =
            crate::reader::install(mock_builder(), crate::reader::ReaderState::open(dir.path()))
                .build(mock_context(noop_assets()))
                .expect("mock app builds");
        let (tx, rx) = mpsc::channel::<Value>();
        app.listen("tauri://drag-drop", move |event| {
            let payload: Value = serde_json::from_str(event.payload()).unwrap();
            let _ = tx.send(payload);
        });

        let paths = vec!["/tmp/one.pdf".to_string(), "/tmp/two.md".to_string()];
        let val = emit_drop_on_app(app.handle(), &paths).expect("emit_drop_on_app succeeds");
        assert_eq!(val["dropped"], 2);
        let received = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("drag-drop event emitted");
        assert_eq!(received["paths"], json!(["/tmp/one.pdf", "/tmp/two.md"]));
        assert_eq!(received["position"], json!({ "x": 0.0, "y": 0.0 }));
    }

    struct EchoBackend;
    impl CtlBackend for EchoBackend {
        fn execute(&self, cmd: CtlCommand) -> Result<Value, String> {
            match cmd {
                CtlCommand::Text { selector } => Ok(json!(format!("text:{selector}"))),
                CtlCommand::Count { .. } => Ok(json!(4)),
                CtlCommand::Eval { js } if js == "boom" => Err("eval boom".into()),
                other => Ok(json!(format!("{other:?}"))),
            }
        }
    }

    #[test]
    fn socket_server_handles_requests_with_0700_dir_and_removes_socket_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = resolve_socket_path(None, dir.path(), 9999);
        let server =
            CtlServer::bind(sock_path.clone(), Arc::new(EchoBackend)).expect("server binds");

        assert!(
            sock_path.exists(),
            "socket file exists while server is alive"
        );
        let parent_mode = std::fs::metadata(sock_path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(parent_mode, 0o700, "socket directory mode must be 0700");

        let ok_reply = send_request(
            &sock_path,
            &Request {
                id: json!("req-1"),
                cmd: "text".into(),
                args: json!([".title"]),
            },
        )
        .expect("send_request succeeds");
        let ok_json: Value = serde_json::to_value(&ok_reply).unwrap();
        assert_eq!(
            ok_json,
            json!({ "id": "req-1", "ok": true, "value": "text:.title" })
        );

        let err_reply = send_request(
            &sock_path,
            &Request {
                id: json!(2),
                cmd: "eval".into(),
                args: json!(["boom"]),
            },
        )
        .expect("send_request returns error reply");
        let err_json: Value = serde_json::to_value(&err_reply).unwrap();
        assert_eq!(
            err_json,
            json!({ "id": 2, "ok": false, "error": "eval boom" })
        );

        let bad_cmd = send_request(
            &sock_path,
            &Request {
                id: json!(3),
                cmd: "not-a-command".into(),
                args: json!([]),
            },
        )
        .expect("send_request returns error reply for bad command");
        assert!(bad_cmd.result.is_err());

        // Also test run_cli with --socket against the running server.
        let code = run_cli(vec![
            "--socket".into(),
            sock_path.to_string_lossy().into_owned(),
            "count".into(),
            "li.draft-chapter".into(),
        ]);
        assert_eq!(code, 0);

        let resize_code = run_cli(vec![
            "--socket".into(),
            sock_path.to_string_lossy().into_owned(),
            "resize".into(),
            "900".into(),
            "700".into(),
        ]);
        assert_eq!(resize_code, 0);

        let fail_code = run_cli(vec![
            "--socket".into(),
            sock_path.to_string_lossy().into_owned(),
            "eval".into(),
            "boom".into(),
        ]);
        assert_eq!(fail_code, 1);

        drop(server);
        assert!(!sock_path.exists(), "socket file is removed on server drop");
    }
}
