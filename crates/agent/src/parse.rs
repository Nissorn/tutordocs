//! NDJSON line parser for `claude -p --output-format stream-json`.
//!
//! Shapes follow the recorded fixtures in `tests/fixtures/`. Unknown event
//! types are ignored so CLI auto-updates do not break the stream, and so are
//! non-JSON lines: success or failure comes from the `result` event alone.

use serde_json::Value;

use crate::logline::tool_detail;
use crate::runner::LineParser;
use crate::types::AgentEvent;

/// Stateful per-job parser: one session id, and assistant text is suppressed
/// once partial-message deltas have been seen so text is never doubled.
#[derive(Default)]
pub struct NdjsonParser {
    session_emitted: bool,
    saw_text_delta: bool,
}

impl NdjsonParser {
    pub fn new() -> Self {
        Self {
            session_emitted: false,
            saw_text_delta: false,
        }
    }

    /// Parse one stdout line into zero or more events.
    pub fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        // A non-JSON line is a CLI diagnostic, such as Qoder's plain-text
        // skill-rename warning, not a job failure. The runner keeps it as
        // detail for a job that ends without a result.
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };

        let mut out = Vec::new();
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "system" if v.get("subtype").and_then(Value::as_str) == Some("init") => {
                if !self.session_emitted {
                    if let Some(id) = v.get("session_id").and_then(Value::as_str) {
                        self.session_emitted = true;
                        out.push(AgentEvent::Session { id: id.to_string() });
                    }
                }
            }
            "stream_event" => {
                let delta = v.pointer("/event/delta");
                if let Some(text) = delta
                    .filter(|d| d.get("type").and_then(Value::as_str) == Some("text_delta"))
                    .and_then(|d| d.get("text"))
                    .and_then(Value::as_str)
                {
                    self.saw_text_delta = true;
                    out.push(AgentEvent::TextDelta(text.to_string()));
                }
            }
            "assistant" => {
                // Deltas for this message already streamed. The snapshot would
                // repeat them, so it is skipped, and the boundary still closes
                // the log line. The flag resets so the next message is its own
                // line.
                let streamed = self.saw_text_delta;
                let mut emitted_text = false;
                if let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) {
                    for block in blocks {
                        match block.get("type").and_then(Value::as_str).unwrap_or("") {
                            "text" if !self.saw_text_delta => {
                                if let Some(text) = block.get("text").and_then(Value::as_str) {
                                    if !text.is_empty() {
                                        out.push(AgentEvent::TextDelta(text.to_string()));
                                        emitted_text = true;
                                    }
                                }
                            }
                            "tool_use" => {
                                if let Some(name) = block.get("name").and_then(Value::as_str) {
                                    let detail = block
                                        .get("input")
                                        .and_then(|input| tool_detail(name, input));
                                    out.push(AgentEvent::ToolUse {
                                        name: name.to_string(),
                                        detail,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if streamed || emitted_text {
                    out.push(AgentEvent::MessageBoundary);
                    self.saw_text_delta = false;
                }
            }
            "result" => {
                let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("");
                let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false)
                    || subtype != "success";
                let text = v.get("result").and_then(Value::as_str).unwrap_or("");
                if is_error {
                    let msg = if !text.is_empty() {
                        text.to_string()
                    } else if let Some(detail) = error_detail(&v) {
                        detail
                    } else if subtype.is_empty() {
                        "result error".to_string()
                    } else {
                        subtype.to_string()
                    };
                    out.push(AgentEvent::Error(msg));
                } else {
                    // A --json-schema job reports its object in
                    // `structured_output`. Without it, `result` is JSON text
                    // for structured jobs and prose for plain ones.
                    let value = match v.get("structured_output") {
                        Some(structured) if !structured.is_null() => structured.clone(),
                        _ => serde_json::from_str(text)
                            .unwrap_or_else(|_| Value::String(text.to_string())),
                    };
                    out.push(AgentEvent::Result { value });
                }
            }
            _ => {}
        }
        out
    }
}

/// The failure detail an error result carries when `result` is empty: an
/// `errors` string array (Qoder) or an `error_message` string.
fn error_detail(result: &Value) -> Option<String> {
    let errors: Vec<&str> = result
        .get("errors")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if !errors.is_empty() {
        return Some(errors.join("; "));
    }
    result
        .get("error_message")
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
}

/// A model reply as a JSON value when it is JSON, otherwise as text. A reply
/// that is exactly one Markdown code fence around JSON counts as JSON, because
/// models add the fence even when told not to. Prose around a fence does not:
/// guessing there would turn a malformed answer into a silent pass.
pub(crate) fn json_or_text(text: &str) -> Value {
    if let Ok(v) = serde_json::from_str(text) {
        return v;
    }
    unfence(text)
        .and_then(|inner| serde_json::from_str(inner).ok())
        .unwrap_or_else(|| Value::String(text.to_string()))
}

fn unfence(text: &str) -> Option<&str> {
    let body = text.trim().strip_prefix("```")?.strip_suffix("```")?;
    // Drop the info string (`json`) on the opening line.
    let (_, inner) = body.split_once('\n')?;
    Some(inner.trim())
}

impl LineParser for NdjsonParser {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        NdjsonParser::parse_line(self, line)
    }
}
