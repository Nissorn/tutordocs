//! A temp library plus a fake `AgentDriver` for the outline and chapter tests.
//! The fake is built in this crate with `JobHandle::scripted` and
//! `EventStream::for_job`. It records the `JobSpec` and what the workdir held,
//! then does what its `ChapterAct` says inside that workdir.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use tutordocs_agent::{
    AgentDriver, AgentEvent, AuthKind, Capabilities, EventStream, JobHandle, JobSpec,
};
use tutordocs_store::Library;

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// What a chapter job's fake agent does. An outline job ignores this and
/// returns the JSON fixture named on [`Fake`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChapterAct {
    /// Write the chapter fixture and a figure into `CHAPTER_DIR`.
    Write,
    /// Write a short `index.md`, then report a failure.
    PartialThenFail,
    /// Write nothing and explain why in a successful reply.
    ReplyOnly,
    /// Report the same timeout the agent runner reports, after a partial write.
    TimedOut,
}

pub struct Fake<'a> {
    pub outline: &'a str,
    pub chapter: ChapterAct,
    /// How long `start` waits before it records the job. A joined open can
    /// subscribe during the wait.
    pub delay: Duration,
}

impl Default for Fake<'static> {
    fn default() -> Self {
        Self {
            outline: "outline.json",
            chapter: ChapterAct::Write,
            delay: Duration::ZERO,
        }
    }
}

/// One job the fake ran.
#[derive(Clone, Debug)]
pub struct Seen {
    pub job: JobSpec,
    /// Relative paths present when the job started. Directories end in `/`.
    pub files_at_start: Vec<String>,
    /// Text of the files in `files_at_start`, keyed by the same relative path.
    pub texts: BTreeMap<String, String>,
}

pub struct FakeDriver {
    outline_file: PathBuf,
    chapter: ChapterAct,
    delay: Duration,
    seen: Mutex<Vec<Seen>>,
}

impl FakeDriver {
    fn new(fake: Fake<'_>) -> Self {
        Self {
            outline_file: fixture(fake.outline),
            chapter: fake.chapter,
            delay: fake.delay,
            seen: Mutex::new(Vec::new()),
        }
    }

    /// Every job started, in order.
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("seen lock").clone()
    }
}

pub struct Env {
    _tmp: tempfile::TempDir,
    pub library: PathBuf,
    pub driver: FakeDriver,
}

/// A fresh library and a fake agent that follows `fake`.
pub fn setup(fake: Fake<'_>) -> Env {
    let tmp = tempfile::tempdir().expect("temp dir");
    let library = tmp.path().join("library");
    Library::create(&library).expect("create library");
    Env {
        _tmp: tmp,
        library,
        driver: FakeDriver::new(fake),
    }
}

/// Every job the fake started, in order.
pub fn ran(env: &Env) -> Vec<Seen> {
    env.driver.seen()
}

/// Every prompt the fake received, in order.
pub fn prompts(env: &Env) -> Vec<String> {
    ran(env).into_iter().map(|seen| seen.job.prompt).collect()
}

/// Every entry directly inside `dir`, sorted. Missing reads as empty.
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

/// Relative paths under `dir`. Directories end in `/`, and file text is kept.
fn snapshot(dir: &Path) -> (Vec<String>, BTreeMap<String, String>) {
    let mut paths = Vec::new();
    let mut texts = BTreeMap::new();
    if dir.is_dir() {
        walk(dir, dir, &mut paths, &mut texts);
    }
    paths.sort();
    (paths, texts)
}

fn walk(root: &Path, dir: &Path, paths: &mut Vec<String>, texts: &mut BTreeMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if is_dir {
            paths.push(format!("{rel}/"));
            walk(root, &path, paths, texts);
        } else {
            paths.push(rel.clone());
            if let Ok(text) = std::fs::read_to_string(&path) {
                texts.insert(rel, text);
            }
        }
    }
}

fn prompt_field<'a>(prompt: &'a str, field: &str) -> Option<&'a str> {
    prompt
        .lines()
        .find_map(|line| line.strip_prefix(field))
        .map(str::trim)
}

fn chapter_output(job: &JobSpec) -> PathBuf {
    let rel = prompt_field(&job.prompt, "CHAPTER_DIR: ").expect("chapter prompt names CHAPTER_DIR");
    job.workdir.join(rel)
}

fn write_chapter(dir: &Path) {
    std::fs::create_dir_all(dir.join("visuals")).unwrap();
    std::fs::copy(fixture("chapter-moves.md"), dir.join("index.md")).unwrap();
    std::fs::write(dir.join("visuals/figure.svg"), "<svg/>\n").unwrap();
}

fn write_partial(dir: &Path) {
    let bytes = std::fs::read(fixture("chapter-moves.md")).unwrap();
    let end = 20.min(bytes.len());
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("index.md"), &bytes[..end]).unwrap();
}

fn outline_events(outline_file: &Path) -> Vec<AgentEvent> {
    let text = std::fs::read_to_string(outline_file).expect("outline fixture");
    let value: serde_json::Value = serde_json::from_str(&text).expect("outline fixture is JSON");
    vec![
        AgentEvent::Session {
            id: "sess-fake".into(),
        },
        AgentEvent::Result { value },
        AgentEvent::Done,
    ]
}

fn chapter_events(act: ChapterAct, dir: &Path) -> Vec<AgentEvent> {
    let session = AgentEvent::Session {
        id: "sess-fake".into(),
    };
    match act {
        ChapterAct::Write => {
            write_chapter(dir);
            vec![
                session,
                AgentEvent::ToolUse {
                    name: "Grep".into(),
                    detail: None,
                },
                AgentEvent::ToolUse {
                    name: "Write".into(),
                    detail: None,
                },
                AgentEvent::TextDelta("Wrote the chapter.".into()),
                AgentEvent::Result {
                    value: "Wrote the chapter.".into(),
                },
                AgentEvent::Done,
            ]
        }
        ChapterAct::PartialThenFail => {
            write_partial(dir);
            vec![
                session,
                AgentEvent::Error("Ran out of turns while writing".into()),
                AgentEvent::Done,
            ]
        }
        ChapterAct::ReplyOnly => vec![
            session,
            AgentEvent::Result {
                value: "I could not find enough material to write this chapter.".into(),
            },
            AgentEvent::Done,
        ],
        ChapterAct::TimedOut => {
            write_partial(dir);
            vec![
                session,
                AgentEvent::Error("job timed out; child killed".into()),
                AgentEvent::Done,
            ]
        }
    }
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
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        let (files_at_start, texts) = snapshot(&job.workdir);
        self.seen.lock().expect("seen lock").push(Seen {
            job: job.clone(),
            files_at_start,
            texts,
        });
        let events = if prompt_field(&job.prompt, "CHAPTER_DIR: ").is_some() {
            chapter_events(self.chapter, &chapter_output(&job))
        } else {
            outline_events(&self.outline_file)
        };
        Ok(JobHandle::scripted(events))
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
