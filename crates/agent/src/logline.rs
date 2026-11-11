//! Fold a driver's events into the lines a job log shows.
//!
//! Text deltas stay separate on the event stream so a live character count
//! can tick. The log waits for [`AgentEvent::MessageBoundary`] and then keeps
//! one text line for that assistant message.

use crate::types::AgentEvent;

/// One log line before it is stamped with a time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Folded {
    Text(String),
    Tool {
        name: String,
        detail: Option<String>,
    },
    Error(String),
}

/// Buffers the current assistant message.
#[derive(Default)]
pub struct LogFold {
    buf: String,
}

impl LogFold {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lines closed by `event`. A text delta only extends the open message.
    pub fn push(&mut self, event: &AgentEvent) -> Vec<Folded> {
        let mut out = Vec::new();
        match event {
            AgentEvent::TextDelta(text) => self.buf.push_str(text),
            AgentEvent::MessageBoundary => self.flush_into(&mut out),
            AgentEvent::ToolUse { name, detail } => {
                self.flush_into(&mut out);
                out.push(Folded::Tool {
                    name: name.clone(),
                    detail: detail.clone(),
                });
            }
            AgentEvent::Error(text) => {
                self.flush_into(&mut out);
                out.push(Folded::Error(text.clone()));
            }
            AgentEvent::Result { .. } | AgentEvent::Done | AgentEvent::Session { .. } => {
                self.flush_into(&mut out);
            }
        }
        out
    }

    /// The trailing message when the stream ends without a boundary.
    pub fn finish(&mut self) -> Option<Folded> {
        if self.buf.is_empty() {
            None
        } else {
            Some(Folded::Text(std::mem::take(&mut self.buf)))
        }
    }

    fn flush_into(&mut self, out: &mut Vec<Folded>) {
        if !self.buf.is_empty() {
            out.push(Folded::Text(std::mem::take(&mut self.buf)));
        }
    }
}

/// Fold a whole stream, including a message that never saw a boundary.
pub fn fold_events(events: &[AgentEvent]) -> Vec<Folded> {
    let mut fold = LogFold::new();
    let mut out = Vec::new();
    for event in events {
        out.extend(fold.push(event));
    }
    if let Some(last) = fold.finish() {
        out.push(last);
    }
    out
}

/// The file path, pattern, or URL a named tool carried, when the object has it.
pub(crate) fn tool_detail(name: &str, input: &serde_json::Value) -> Option<String> {
    let keys: &[&str] = match name {
        "Read" | "Write" | "Edit" => &["file_path", "path"],
        "Grep" | "Glob" => &["pattern"],
        "WebFetch" => &["url"],
        _ => return None,
    };
    for key in keys {
        if let Some(text) = input.get(*key).and_then(serde_json::Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}
