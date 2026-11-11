//! A temp library plus a fake `AgentDriver` built outside the agent crate. The
//! fake acts like an agent told to fetch a page: it reads OUTPUT_FILE from the
//! prompt and does what its `Act` says inside the job's workdir.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tutordocs_agent::{
    AgentDriver, AgentEvent, AuthKind, Capabilities, EventStream, JobHandle, JobSpec,
};
use tutordocs_store::Library;

pub const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/example-article.md"
);

/// What the fake agent does with a job.
#[derive(Clone, Copy, Debug)]
pub enum Act {
    /// Save the fixture page to OUTPUT_FILE and report success.
    Write,
    /// Save part of the page, then report a failed fetch.
    PartialThenFail,
    /// Save nothing and explain why in a successful reply.
    ReplyOnly,
    /// Save a file holding only whitespace.
    BlankFile,
    /// Fail to start, as a missing CLI would.
    CannotStart,
}

/// One job the fake ran: the spec, and what its workdir held when it started.
#[derive(Clone, Debug)]
pub struct Seen {
    pub job: JobSpec,
    pub workdir_at_start: Vec<String>,
}

pub struct FakeDriver {
    act: Act,
    seen: Mutex<Vec<Seen>>,
}

impl FakeDriver {
    pub fn new(act: Act) -> Self {
        Self {
            act,
            seen: Mutex::new(Vec::new()),
        }
    }

    /// Every job started, in order.
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

fn prompt_field<'a>(prompt: &'a str, field: &str) -> &'a str {
    prompt
        .lines()
        .find_map(|l| l.strip_prefix(field))
        .unwrap_or_else(|| panic!("prompt has no {field} line"))
}

#[tutordocs_agent::async_trait]
impl AgentDriver for FakeDriver {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            headless: true,
            stream_json: true,
            structured_output: true,
            resume: true,
            system_append: true,
            tool_allowlist: true,
            read_only: true,
            model: false,
            effort: false,
            auth: AuthKind::SubscriptionOAuth,
        }
    }

    async fn start(&self, job: JobSpec) -> tutordocs_agent::anyhow::Result<JobHandle> {
        if let Act::CannotStart = self.act {
            tutordocs_agent::anyhow::bail!("spawning fake failed: No such file or directory");
        }
        self.seen.lock().unwrap().push(Seen {
            workdir_at_start: names_in(&job.workdir),
            job: job.clone(),
        });
        let out = job.workdir.join(prompt_field(&job.prompt, "OUTPUT_FILE: "));
        let page = std::fs::read_to_string(FIXTURE).unwrap();
        let session = AgentEvent::Session {
            id: "sess-fake".into(),
        };
        let result = |text: &str| AgentEvent::Result { value: text.into() };
        let events = match self.act {
            Act::Write => {
                std::fs::write(&out, page).unwrap();
                vec![session, result("Saved the page.")]
            }
            Act::PartialThenFail => {
                std::fs::write(&out, &page[..40]).unwrap();
                vec![
                    session,
                    AgentEvent::Error("Could not fetch the page: HTTP 404 Not Found".into()),
                ]
            }
            Act::ReplyOnly => vec![
                session,
                result("I could not reach that page: connection refused."),
            ],
            Act::BlankFile => {
                std::fs::write(&out, "  \n\n").unwrap();
                vec![session, result("Saved the page.")]
            }
            Act::CannotStart => unreachable!(),
        };
        Ok(JobHandle::scripted(
            events.into_iter().chain([AgentEvent::Done]),
        ))
    }

    async fn resume(
        &self,
        _session: &str,
        job: JobSpec,
    ) -> tutordocs_agent::anyhow::Result<JobHandle> {
        self.start(job).await
    }

    fn events(&self, h: &JobHandle) -> EventStream {
        EventStream::for_job(h)
    }

    async fn cancel(&self, _h: &JobHandle) -> tutordocs_agent::anyhow::Result<()> {
        Ok(())
    }
}

pub struct Env {
    _tmp: tempfile::TempDir,
    pub library: PathBuf,
    pub driver: FakeDriver,
}

/// 2023-11-14T22:13:20Z
pub fn fetched_at() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

/// A fresh library and a fake agent that does `act`.
pub fn setup(act: Act) -> Env {
    let tmp = tempfile::tempdir().expect("temp dir");
    let library = tmp.path().join("library");
    Library::create(&library).expect("create library");
    Env {
        _tmp: tmp,
        library,
        driver: FakeDriver::new(act),
    }
}

/// Every file name directly inside `dir`, sorted.
pub fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}
