//! What a job may do, enforced on the CLI's command line: the tool allowlist
//! and read-only mode. A driver that cannot enforce a constraint the job asks
//! for refuses the job, so no job believes it is constrained when it is not.

use anyhow::bail;

use crate::types::{Capabilities, JobSpec};

/// Claude-shaped tools that change files. Claude Code checks them against
/// `Edit(path)` rules, and a bare allow rule for one approves a write to any
/// path, so they are never pre-approved. The edit-capable permission mode
/// approves them inside the working directory only.
const FILE_EDITING_TOOLS: &[&str] = &["Write", "Edit", "MultiEdit", "NotebookEdit"];

/// Refuse a job whose read-only mode or tool allowlist the probed CLI cannot
/// enforce.
pub(crate) fn require_enforceable(
    name: &str,
    job: &JobSpec,
    caps: &Capabilities,
) -> anyhow::Result<()> {
    if job.read_only && !caps.read_only {
        bail!("probed {name} CLI has no confirmed read-only mode; refusing a read-only job");
    }
    if job.allowed_tools.is_some() && !caps.tool_allowlist {
        bail!("probed {name} CLI cannot enforce a tool allowlist; refusing the job");
    }
    Ok(())
}

/// True when the job may change files. An absent allowlist leaves the CLI's
/// whole default tool set, which can edit, so it counts as editing. An empty
/// allowlist names no tool.
pub(crate) fn allowlist_edits_files(job: &JobSpec) -> bool {
    job.allowed_tools.as_deref().is_none_or(|tools| {
        tools
            .iter()
            .any(|tool| FILE_EDITING_TOOLS.contains(&tool.as_str()))
    })
}

/// The argv for a job's allowlist: `tools_flag` limits the tool set to the
/// list, and `approve_flag` pre-approves the ones that are not file-editing.
/// Both lists are comma-joined in one value, the form claude's `--help` shows.
pub(crate) fn tool_args(job: &JobSpec, tools_flag: &str, approve_flag: &str) -> Vec<String> {
    let Some(tools) = &job.allowed_tools else {
        return Vec::new();
    };
    let mut args = vec![tools_flag.to_string(), tools.join(",")];
    let approved: Vec<&str> = tools
        .iter()
        .map(String::as_str)
        .filter(|t| !FILE_EDITING_TOOLS.contains(t))
        .collect();
    if !approved.is_empty() {
        args.push(approve_flag.to_string());
        args.push(approved.join(","));
    }
    args
}
