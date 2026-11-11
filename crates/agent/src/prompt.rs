//! Prompt shaping for the fields a CLI cannot take natively.
//!
//! A driver passes a job's system text and JSON schema as flags when the
//! probed `--help` advertises them. When it does not, the same intent goes
//! through the prompt body, so a job never silently loses its constraints.

use crate::types::{Capabilities, JobSpec};

/// The prompt body to send: the job's prompt, with system text prepended when
/// the CLI has no native flag for it and a schema instruction appended when
/// the CLI cannot enforce a schema itself.
pub(crate) fn shaped_prompt(job: &JobSpec, caps: &Capabilities) -> String {
    let mut prompt = job.prompt.clone();
    if let Some(schema) = job.json_schema.as_ref().filter(|_| !caps.structured_output) {
        prompt = format!(
            "{prompt}\n\nReply with one JSON value that conforms to this JSON Schema and nothing else: no prose and no Markdown code fence.\nJSON Schema: {schema}"
        );
    }
    if let Some(system) = job.system_append.as_ref().filter(|_| !caps.system_append) {
        prompt = format!(
            "Instructions for this task (follow them for the whole reply):\n{system}\n\nTask:\n{prompt}"
        );
    }
    prompt
}
