//! The probed, installed CLI that the Qoder and agy drivers share: where it
//! lives, what `--version` said, what `--help` advertised, and where to tap
//! raw stdout. Probing happens once at construction and is cached here.

use std::path::PathBuf;

use anyhow::bail;

use crate::policy::require_enforceable;
use crate::probe::run_probe;
use crate::runner::{self, LineParser};
use crate::types::{Capabilities, JobHandle, JobSpec};

pub(crate) struct Cli {
    pub(crate) binary: PathBuf,
    pub(crate) version: String,
    pub(crate) caps: Capabilities,
    pub(crate) tap: Option<PathBuf>,
}

impl Cli {
    /// Run `--version` and `--help` once and read capabilities with the
    /// driver's pure `caps_from_help`.
    pub(crate) fn probe(
        binary: PathBuf,
        caps_from_help: fn(&str) -> Capabilities,
    ) -> anyhow::Result<Self> {
        let probe = run_probe(&binary)?;
        Ok(Self {
            caps: caps_from_help(&probe.help),
            version: probe.version,
            binary,
            tap: None,
        })
    }

    /// Refuse a job the probed CLI cannot run, naming the missing flag. Fields
    /// a driver can emulate through the prompt are not checked here; a
    /// read-only mode and a tool allowlist cannot be emulated, so they are.
    pub(crate) fn require(
        &self,
        name: &str,
        job: &JobSpec,
        resume: Option<&str>,
        resume_flag: &str,
    ) -> anyhow::Result<()> {
        if !self.caps.headless || !self.caps.stream_json {
            bail!("probed {name} CLI lacks --print/stream-json support; cannot drive headlessly");
        }
        if resume.is_some() && !self.caps.resume {
            bail!("probed {name} CLI lacks {resume_flag}");
        }
        require_enforceable(name, job, &self.caps)
    }

    pub(crate) async fn launch(
        &self,
        label: &'static str,
        extra_scrub: &'static [&'static str],
        job: &JobSpec,
        args: Vec<String>,
        parser: impl LineParser,
    ) -> anyhow::Result<JobHandle> {
        runner::launch(
            runner::Launch {
                label,
                binary: &self.binary,
                args,
                workdir: &job.workdir,
                extra_scrub,
                tap: self.tap.as_deref(),
                timeout: job.timeout,
                schema: job.json_schema.as_ref(),
                cleanup_dir: None,
            },
            parser,
        )
        .await
    }
}
