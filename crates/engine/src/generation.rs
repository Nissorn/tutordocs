//! What outline and chapter generation report, and the one loop that drains a
//! job's events. The UI shows these messages.

use std::sync::Arc;

use tokio::sync::Notify;
use tutordocs_agent::{AgentDriver, AgentEvent, JobSpec};
use tutordocs_store::StoreError;

use crate::joblog::{flag_value, redact_command, with_log, JobKind, JobOutcome, JobStart};

const MAX_REASON_CHARS: usize = 500;

/// Everything outline or chapter generation can fail with. Every variant
/// leaves `course.json` and `chapters/` as they were. `Clone`, because every
/// open that joined one chapter job receives the same outcome.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GenerationError {
    /// The request itself is unusable, such as an empty topic.
    #[error("{0}")]
    InvalidRequest(String),

    /// The library folder could not be read or written.
    #[error("{0}")]
    Store(String),

    /// The agent driver could not start the job.
    #[error("could not start the agent: {0}")]
    Driver(String),

    /// The job reported an error or timed out.
    #[error("the agent job failed: {0}")]
    JobFailed(String),

    /// The outline the agent returned breaks a rule the course depends on.
    #[error("the agent's outline was not usable: {0}")]
    InvalidOutline(String),

    /// The job ended cleanly but wrote no chapter. Carries what the agent said.
    #[error("the agent finished without writing the chapter: {0}")]
    NothingWritten(String),
}

impl From<StoreError> for GenerationError {
    fn from(e: StoreError) -> Self {
        Self::Store(e.to_string())
    }
}

/// What a job did, as the UI shows it while a chapter is being written.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Progress {
    /// The agent process is running.
    Started,
    /// The agent called a tool, such as a search or a file write.
    Tool { name: String },
    /// The agent has said this many characters so far.
    Text { chars: usize },
}

/// Run one job to its end, reporting progress, and return the result value or
/// the spoken text. A reported error is a failure whatever else the job did.
/// When the shell has installed a job log, this records the redacted command
/// and every line.
pub(crate) async fn run_job(
    driver: &dyn AgentDriver,
    job: JobSpec,
    on_progress: &(dyn Fn(Progress) + Send + Sync),
    kind: JobKind,
    course_id: &str,
) -> Result<Reply, GenerationError> {
    let prompt = job.prompt.clone();
    let system = job.system_append.clone();
    let handle = match driver.start(job).await {
        Ok(handle) => handle,
        Err(error) => {
            let message = clip(&format!("{error:#}"));
            note_unstarted(driver.id(), kind, course_id, &message);
            return Err(GenerationError::Driver(message));
        }
    };
    on_progress(Progress::Started);
    let wake = Arc::new(Notify::new());
    let command = redact_command(&handle.command, &prompt, system.as_deref());
    let armed = with_log(|log| {
        let wake = Arc::clone(&wake);
        log.begin(JobStart {
            course_id: course_id.to_string(),
            kind,
            driver: driver.id().to_string(),
            model: flag_value(&handle.command, "--model"),
            effort: flag_value(&handle.command, "--effort"),
            command,
            cancel: handle.cancel_flag(),
            wake: Arc::new(move || wake.notify_one()),
        })
    });
    let mut events = driver.events(&handle);
    let mut first_error: Option<String> = None;
    let mut result: Option<serde_json::Value> = None;
    let mut spoken = String::new();
    let cancelled = loop {
        if handle.is_cancelled() {
            break true;
        }
        let event = if armed.is_some() {
            tokio::select! {
                biased;
                _ = wake.notified() => continue,
                event = events.recv() => event,
            }
        } else {
            events.recv().await
        };
        let Some(event) = event else {
            break handle.is_cancelled();
        };
        if let Some(id) = armed.as_ref() {
            with_log(|log| log.record(id, &event));
        }
        match event {
            AgentEvent::Error(message) => {
                first_error.get_or_insert(message);
            }
            AgentEvent::Result { value } => result = Some(value),
            AgentEvent::TextDelta(text) => {
                spoken.push_str(&text);
                on_progress(Progress::Text {
                    chars: spoken.chars().count(),
                });
            }
            AgentEvent::ToolUse { name, .. } => on_progress(Progress::Tool { name }),
            AgentEvent::Done => break handle.is_cancelled(),
            AgentEvent::MessageBoundary | AgentEvent::Session { .. } => {}
        }
    };
    if cancelled {
        let _ = driver.cancel(&handle).await;
        finish(armed.as_deref(), JobOutcome::Cancelled);
        return Err(GenerationError::JobFailed(
            "the agent job was cancelled".to_string(),
        ));
    }
    if let Some(message) = first_error {
        finish(armed.as_deref(), JobOutcome::Failed);
        return Err(GenerationError::JobFailed(clip(&message)));
    }
    finish(armed.as_deref(), JobOutcome::Succeeded);
    Ok(Reply { result, spoken })
}

fn finish(id: Option<&str>, outcome: JobOutcome) {
    let Some(id) = id else {
        return;
    };
    with_log(|log| log.finish(id, outcome));
}

/// A spawn that never produced argv still shows up, as a failed job.
fn note_unstarted(driver: &str, kind: JobKind, course_id: &str, message: &str) {
    with_log(|log| {
        let id = log.begin(JobStart {
            course_id: course_id.to_string(),
            kind,
            driver: driver.to_string(),
            model: None,
            effort: None,
            command: String::new(),
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            wake: Arc::new(|| {}),
        });
        log.record(&id, &AgentEvent::Error(message.to_string()));
        log.finish(&id, JobOutcome::Failed);
    });
}

/// How a clean job ended.
pub(crate) struct Reply {
    pub result: Option<serde_json::Value>,
    pub spoken: String,
}

impl Reply {
    /// What the agent said last, for a message that explains a missing write.
    pub fn said(&self) -> String {
        match &self.result {
            Some(serde_json::Value::String(text)) => clip(text),
            Some(value) => clip(&value.to_string()),
            None => clip(&self.spoken),
        }
    }
}

pub(crate) fn clip(text: &str) -> String {
    text.trim().chars().take(MAX_REASON_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tutordocs_agent::{
        AgentDriver, AgentEvent, AuthKind, Capabilities, EventStream, JobHandle, JobSpec,
    };

    use super::*;
    use crate::joblog::{job_scope, JobLog};

    const PROMPT: &str = "hello there";
    const SYSTEM: &str = "be brief";
    const REDACTED: &str =
        "claude -p <prompt, 11 chars> --append-system-prompt <system, 8 chars> --model sonnet";

    #[derive(Default)]
    struct Mem {
        starts: Mutex<Vec<JobStartView>>,
        events: Mutex<Vec<AgentEvent>>,
        outcomes: Mutex<Vec<JobOutcome>>,
    }

    struct JobStartView {
        command: String,
        model: Option<String>,
        effort: Option<String>,
        kind: JobKind,
        course_id: String,
        driver: String,
    }

    impl JobLog for Mem {
        fn begin(&self, start: JobStart) -> String {
            self.starts.lock().unwrap().push(JobStartView {
                command: start.command,
                model: start.model,
                effort: start.effort,
                kind: start.kind,
                course_id: start.course_id,
                driver: start.driver,
            });
            "job-1".into()
        }

        fn record(&self, _id: &str, event: &AgentEvent) {
            self.events.lock().unwrap().push(event.clone());
        }

        fn finish(&self, _id: &str, outcome: JobOutcome) {
            self.outcomes.lock().unwrap().push(outcome);
        }
    }

    fn caps() -> Capabilities {
        Capabilities {
            headless: true,
            stream_json: true,
            structured_output: true,
            resume: true,
            system_append: true,
            tool_allowlist: true,
            read_only: true,
            model: true,
            effort: true,
            auth: AuthKind::SubscriptionOAuth,
        }
    }

    /// A driver whose handle carries the argv a real spawn would have used.
    struct Script {
        events: Vec<AgentEvent>,
        cancel: bool,
        refuse: bool,
    }

    #[tutordocs_agent::async_trait]
    impl AgentDriver for Script {
        fn id(&self) -> &'static str {
            "claude"
        }

        fn capabilities(&self) -> Capabilities {
            caps()
        }

        async fn start(&self, job: JobSpec) -> tutordocs_agent::anyhow::Result<JobHandle> {
            if self.refuse {
                tutordocs_agent::anyhow::bail!("no binary");
            }
            let mut command = vec!["claude".into(), "-p".into(), job.prompt.clone()];
            if let Some(system) = &job.system_append {
                command.push("--append-system-prompt".into());
                command.push(system.clone());
            }
            command.extend(["--model".into(), "sonnet".into()]);
            let handle = JobHandle::scripted_with_command(command, self.events.clone());
            if self.cancel {
                handle.request_cancel();
            }
            Ok(handle)
        }

        async fn resume(
            &self,
            _: &str,
            job: JobSpec,
        ) -> tutordocs_agent::anyhow::Result<JobHandle> {
            self.start(job).await
        }

        fn events(&self, handle: &JobHandle) -> EventStream {
            EventStream::for_job(handle)
        }

        async fn cancel(&self, _: &JobHandle) -> tutordocs_agent::anyhow::Result<()> {
            Ok(())
        }
    }

    fn spec() -> JobSpec {
        let mut job = JobSpec::new(PROMPT, ".");
        job.system_append = Some(SYSTEM.into());
        job
    }

    async fn run(driver: &Script) -> (Arc<Mem>, Result<Reply, GenerationError>) {
        let log = Arc::new(Mem::default());
        let result = job_scope(log.clone() as Arc<dyn JobLog>, async {
            run_job(driver, spec(), &|_| {}, JobKind::Outline, "lib").await
        })
        .await;
        (log, result)
    }

    #[tokio::test]
    async fn a_scripted_job_records_the_spawned_command_with_the_prompt_replaced() {
        let driver = Script {
            events: vec![
                AgentEvent::TextDelta("Hel".into()),
                AgentEvent::TextDelta("lo".into()),
                AgentEvent::MessageBoundary,
                AgentEvent::ToolUse {
                    name: "Read".into(),
                    detail: Some("sources/notes.md".into()),
                },
                AgentEvent::Result {
                    value: serde_json::json!("ok"),
                },
                AgentEvent::Done,
            ],
            cancel: false,
            refuse: false,
        };
        let (log, result) = run(&driver).await;
        let reply = result.expect("the job succeeds");
        assert_eq!(reply.spoken, "Hello");
        let start = &log.starts.lock().unwrap()[0];
        assert_eq!(start.command, REDACTED);
        assert!(!start.command.contains(PROMPT));
        assert!(!start.command.contains(SYSTEM));
        assert_eq!(start.model.as_deref(), Some("sonnet"));
        assert_eq!(start.effort, None);
        assert_eq!(start.kind, JobKind::Outline);
        assert_eq!(start.course_id, "lib");
        assert_eq!(start.driver, "claude");
        let events = log.events.lock().unwrap();
        assert!(matches!(events[0], AgentEvent::TextDelta(ref text) if text == "Hel"));
        assert!(matches!(events[1], AgentEvent::TextDelta(ref text) if text == "lo"));
        assert!(matches!(events[2], AgentEvent::MessageBoundary));
        assert_eq!(*log.outcomes.lock().unwrap(), [JobOutcome::Succeeded]);
    }

    #[tokio::test]
    async fn a_failing_scripted_job_ends_failed_and_keeps_the_error() {
        let driver = Script {
            events: vec![
                AgentEvent::Error("quota exhausted".into()),
                AgentEvent::Done,
            ],
            cancel: false,
            refuse: false,
        };
        let (log, result) = run(&driver).await;
        let Err(GenerationError::JobFailed(message)) = result else {
            panic!("expected a failed job");
        };
        assert!(message.contains("quota exhausted"), "{message}");
        assert_eq!(log.starts.lock().unwrap()[0].command, REDACTED);
        assert!(log
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, AgentEvent::Error(text) if text == "quota exhausted")));
        assert_eq!(*log.outcomes.lock().unwrap(), [JobOutcome::Failed]);
    }

    #[tokio::test]
    async fn a_cancelled_scripted_job_ends_cancelled_without_an_error_line() {
        let driver = Script {
            events: vec![
                AgentEvent::Error("should not be logged".into()),
                AgentEvent::Done,
            ],
            cancel: true,
            refuse: false,
        };
        let (log, result) = run(&driver).await;
        let Err(GenerationError::JobFailed(message)) = result else {
            panic!("expected cancellation");
        };
        assert!(message.contains("cancelled"), "{message}");
        assert!(log.events.lock().unwrap().is_empty());
        assert_eq!(*log.outcomes.lock().unwrap(), [JobOutcome::Cancelled]);
    }

    #[tokio::test]
    async fn a_driver_that_cannot_start_is_recorded_failed_with_an_empty_command() {
        let driver = Script {
            events: Vec::new(),
            cancel: false,
            refuse: true,
        };
        let (log, result) = run(&driver).await;
        assert!(matches!(result, Err(GenerationError::Driver(_))));
        let start = &log.starts.lock().unwrap()[0];
        assert_eq!(start.command, "");
        assert!(log
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, AgentEvent::Error(_))));
        assert_eq!(*log.outcomes.lock().unwrap(), [JobOutcome::Failed]);
    }

    /// Holds the stream open so the job waits until the log raises cancel.
    struct Hold(Mutex<Option<tokio::sync::mpsc::Sender<AgentEvent>>>);

    #[tutordocs_agent::async_trait]
    impl AgentDriver for Hold {
        fn id(&self) -> &'static str {
            "claude"
        }

        fn capabilities(&self) -> Capabilities {
            caps()
        }

        async fn start(&self, job: JobSpec) -> tutordocs_agent::anyhow::Result<JobHandle> {
            let (handle, tx) =
                JobHandle::open_stream(vec!["claude".into(), "-p".into(), job.prompt.clone()]);
            *self.0.lock().unwrap() = Some(tx);
            Ok(handle)
        }

        async fn resume(
            &self,
            _: &str,
            job: JobSpec,
        ) -> tutordocs_agent::anyhow::Result<JobHandle> {
            self.start(job).await
        }

        fn events(&self, handle: &JobHandle) -> EventStream {
            EventStream::for_job(handle)
        }

        async fn cancel(&self, _: &JobHandle) -> tutordocs_agent::anyhow::Result<()> {
            Ok(())
        }
    }

    struct Waking(Mutex<Vec<JobOutcome>>);

    impl JobLog for Waking {
        fn begin(&self, start: JobStart) -> String {
            let flag = Arc::clone(&start.cancel);
            let wake = Arc::clone(&start.wake);
            std::thread::spawn(move || {
                flag.store(true, Ordering::SeqCst);
                wake();
            });
            "job-1".into()
        }

        fn record(&self, _: &str, _: &AgentEvent) {}

        fn finish(&self, _: &str, outcome: JobOutcome) {
            self.0.lock().unwrap().push(outcome);
        }
    }

    #[tokio::test]
    async fn a_cancel_raised_on_the_log_ends_the_waiting_job() {
        let driver = Hold(Mutex::new(None));
        let log = Arc::new(Waking(Mutex::new(Vec::new())));
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            job_scope(log.clone() as Arc<dyn JobLog>, async {
                run_job(&driver, spec(), &|_| {}, JobKind::Chapter, "lib").await
            })
            .await
        })
        .await;
        let Err(GenerationError::JobFailed(message)) = result.expect("cancel woke the job") else {
            panic!("the waiting job should end cancelled");
        };
        assert!(message.contains("cancelled"), "{message}");
        assert_eq!(*log.0.lock().unwrap(), [JobOutcome::Cancelled]);
    }
}
