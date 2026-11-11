//! In-memory log of every agent job. The last 50 records, each capped at 2000
//! lines. Nothing here is written into the course folder.
//!
//! A line that arrives when a job already holds 2000 lines replaces the last
//! line with `truncated`. The status field still moves when the job ends, so
//! the record shows how it finished even after the line list stopped growing.

use tauri::State;
pub use tutordocs_engine::{JobRecord, JobRegistry};

/// Every record, oldest first.
#[tauri::command]
pub fn list_agent_jobs(jobs: State<'_, JobRegistry>) -> Vec<JobRecord> {
    jobs.list()
}

/// One record. An unknown id is an error.
#[tauri::command]
pub fn agent_job(jobs: State<'_, JobRegistry>, id: String) -> Result<JobRecord, String> {
    jobs.get(&id)
}

/// Ask a running job to stop. An id that is missing or already finished is an error.
#[tauri::command]
pub fn cancel_agent_job(jobs: State<'_, JobRegistry>, id: String) -> Result<(), String> {
    jobs.cancel(&id)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tutordocs_agent::{
        AgentDriver, AgentEvent, AuthKind, Capabilities, EventStream, JobHandle, JobSpec,
    };
    use tutordocs_engine::{JobKind, JobLog, JobOutcome, JobStart, JobStatus, LineKind};
    use tutordocs_store::Library;

    use super::*;
    use crate::generate::GenerationState;
    use crate::reader::{install, ReaderState};

    fn blank_start(course_id: &str) -> JobStart {
        JobStart {
            course_id: course_id.to_string(),
            kind: JobKind::Outline,
            driver: "claude".to_string(),
            model: None,
            effort: None,
            command: "claude -p <prompt, 4 chars>".to_string(),
            cancel: Arc::new(AtomicBool::new(false)),
            wake: Arc::new(|| {}),
        }
    }

    #[test]
    fn the_registry_drops_the_oldest_record_past_fifty() {
        let jobs = JobRegistry::new();
        for n in 1..=51 {
            jobs.begin(blank_start(&format!("c{n}")));
        }
        let listed = jobs.list();
        assert_eq!(listed.len(), 50);
        assert_eq!(listed[0].id, "job-2");
        assert_eq!(listed[0].course_id, "c2");
        assert_eq!(listed.last().unwrap().id, "job-51");
        assert!(jobs.get("job-1").is_err());
    }

    #[test]
    fn a_job_past_two_thousand_lines_ends_on_truncated_and_keeps_its_status() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let slot = seen.clone();
        let jobs = JobRegistry::new();
        jobs.attach_emit(Arc::new(move |name, payload| {
            slot.lock().unwrap().push((name.to_string(), payload));
        }));
        let id = jobs.begin(blank_start("lib"));
        for n in 0..1999 {
            jobs.record(&id, &AgentEvent::Error(format!("e{n}")));
        }
        let full = jobs.get(&id).unwrap();
        assert_eq!(full.lines.len(), 2000);
        assert_eq!(full.lines.last().unwrap().text, "e1998");

        jobs.record(&id, &AgentEvent::Error("overflow".into()));
        jobs.record(&id, &AgentEvent::Error("still more".into()));
        jobs.finish(&id, JobOutcome::Succeeded);

        let record = jobs.get(&id).unwrap();
        assert_eq!(record.lines.len(), 2000);
        let last = record.lines.last().unwrap();
        assert_eq!(last.kind, LineKind::Status);
        assert_eq!(last.text, "truncated");
        assert_eq!(record.status, JobStatus::Succeeded);
        assert!(record.ended_at.is_some());
        assert!(!record.lines.iter().any(|line| line.text == "overflow"));
        assert!(!record.lines.iter().any(|line| line.text == "succeeded"));

        let events = seen.lock().unwrap();
        let truncated = events
            .iter()
            .filter(|(name, payload)| {
                name == "agent-job-line" && payload["line"]["text"] == "truncated"
            })
            .count();
        assert_eq!(truncated, 1);
        assert!(events.iter().any(|(name, payload)| {
            name == "agent-job-status" && payload["status"] == "succeeded"
        }));
    }

    #[test]
    fn cancel_sets_the_flag_of_a_running_job_only() {
        let jobs = JobRegistry::new();
        let flag = Arc::new(AtomicBool::new(false));
        let mut start = blank_start("lib");
        start.cancel = Arc::clone(&flag);
        let id = jobs.begin(start);
        jobs.cancel(&id).unwrap();
        assert!(flag.load(Ordering::SeqCst));
        jobs.finish(&id, JobOutcome::Cancelled);
        let record = jobs.get(&id).unwrap();
        assert_eq!(record.status, JobStatus::Cancelled);
        assert_eq!(record.lines.last().unwrap().text, "cancelled");
        assert!(jobs.cancel(&id).unwrap_err().contains("not running"));
        assert!(jobs.cancel("job-missing").unwrap_err().contains("no job"));
    }

    #[test]
    fn text_deltas_of_one_message_become_one_line_and_a_tool_keeps_its_detail() {
        let jobs = JobRegistry::new();
        let id = jobs.begin(blank_start("lib"));
        jobs.record(&id, &AgentEvent::TextDelta("Hel".into()));
        jobs.record(&id, &AgentEvent::TextDelta("lo".into()));
        jobs.record(&id, &AgentEvent::MessageBoundary);
        jobs.record(
            &id,
            &AgentEvent::ToolUse {
                name: "Read".into(),
                detail: Some("sources/notes.md".into()),
            },
        );
        jobs.finish(&id, JobOutcome::Succeeded);
        let texts: Vec<_> = jobs
            .get(&id)
            .unwrap()
            .lines
            .into_iter()
            .map(|line| (line.kind, line.text))
            .collect();
        assert_eq!(
            texts,
            vec![
                (LineKind::Status, "running".into()),
                (LineKind::Text, "Hello".into()),
                (LineKind::Tool, "Read sources/notes.md".into()),
                (LineKind::Status, "succeeded".into()),
            ]
        );
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
            effort: false,
            auth: AuthKind::SubscriptionOAuth,
        }
    }

    enum Script {
        Ok,
        Fail,
        Cancel,
    }

    struct Scripted(Script);

    #[async_trait::async_trait]
    impl AgentDriver for Scripted {
        fn id(&self) -> &'static str {
            "claude"
        }

        fn capabilities(&self) -> Capabilities {
            caps()
        }

        async fn start(&self, job: JobSpec) -> anyhow::Result<JobHandle> {
            let command = vec![
                "claude".into(),
                "-p".into(),
                job.prompt.clone(),
                "--model".into(),
                "sonnet".into(),
            ];
            let outline = json!({
                "title": "Rust ownership",
                "objectives": ["Explain moves"],
                "chapters": [{
                    "id": "moves",
                    "title": "Moves",
                    "summary": "A move"
                }]
            });
            let events = match self.0 {
                Script::Ok => vec![
                    AgentEvent::TextDelta("Hel".into()),
                    AgentEvent::TextDelta("lo".into()),
                    AgentEvent::MessageBoundary,
                    AgentEvent::ToolUse {
                        name: "Read".into(),
                        detail: Some("sources/notes.md".into()),
                    },
                    AgentEvent::Result { value: outline },
                    AgentEvent::Done,
                ],
                Script::Fail => vec![
                    AgentEvent::Error("quota exhausted".into()),
                    AgentEvent::Done,
                ],
                Script::Cancel => vec![
                    AgentEvent::Error("should not be logged".into()),
                    AgentEvent::Done,
                ],
            };
            let handle = JobHandle::scripted_with_command(command, events);
            if matches!(self.0, Script::Cancel) {
                handle.request_cancel();
            }
            Ok(handle)
        }

        async fn resume(&self, _: &str, job: JobSpec) -> anyhow::Result<JobHandle> {
            self.start(job).await
        }

        fn events(&self, handle: &JobHandle) -> EventStream {
            EventStream::for_job(handle)
        }

        async fn cancel(&self, _: &JobHandle) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn library() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("lib");
        Library::create(&root).unwrap();
        (tmp, root)
    }

    fn agents(root: &std::path::Path, script: Script) -> crate::agents::AgentsState {
        let driver: Arc<dyn AgentDriver> = Arc::new(Scripted(script));
        crate::agents::AgentsState::new(
            root,
            Box::new(move || {
                vec![tutordocs_agent::registry::Detected {
                    status: tutordocs_agent::registry::DriverStatus {
                        id: "claude".to_string(),
                        available: true,
                        binary: None,
                        version: Some("scripted".into()),
                        capabilities: Some(driver.capabilities()),
                        reason: None,
                    },
                    driver: Some(driver.clone()),
                }]
            }),
        )
    }

    /// Several commands on one app, so they share one registry.
    fn session(
        root: &std::path::Path,
        script: Script,
        body: impl FnOnce(&dyn Fn(&str, Value) -> Result<Value, Value>),
    ) {
        let agents = agents(root, script);
        let app = install(mock_builder(), ReaderState::open(root))
            .manage(GenerationState::new(root.to_path_buf(), agents))
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds");
        let call = |cmd: &str, args: Value| {
            get_ipc_response(
                &webview,
                InvokeRequest {
                    cmd: cmd.into(),
                    callback: CallbackFn(0),
                    error: CallbackFn(1),
                    url: "tauri://localhost".parse().unwrap(),
                    body: InvokeBody::Json(args),
                    headers: Default::default(),
                    invoke_key: INVOKE_KEY.to_string(),
                },
            )
            .map(|response| response.deserialize::<Value>().expect("response is JSON"))
        };
        body(&call);
    }

    fn lines_of(record: &Value) -> Vec<(&str, &str)> {
        record["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| {
                (
                    line["kind"].as_str().unwrap(),
                    line["text"].as_str().unwrap(),
                )
            })
            .collect()
    }

    fn assert_placeholder(command: &str) {
        assert!(command.starts_with("claude -p <prompt, "), "{command}");
        assert!(command.contains(" chars> --model sonnet"), "{command}");
        assert!(!command.contains("TOPIC:"), "{command}");
    }

    #[test]
    fn a_scripted_outline_reads_back_with_the_prompt_placeholder_and_ordered_lines() {
        let (_tmp, root) = library();
        session(&root, Script::Ok, |call| {
            let outline = call(
                "generate_outline",
                json!({"courseId": "lib", "topic": "Rust ownership", "sources": []}),
            )
            .expect("the outline job succeeds");
            assert_eq!(outline["id"], "lib");
            let listed = call("list_agent_jobs", json!({})).expect("the list is readable");
            let listed = listed.as_array().unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0]["id"], "job-1");
            let record = call("agent_job", json!({"id": "job-1"})).unwrap();
            assert_eq!(record["courseId"], "lib");
            assert_eq!(record["kind"], "outline");
            assert_eq!(record["driver"], "claude");
            assert_eq!(record["model"], "sonnet");
            assert!(record["effort"].is_null());
            assert_eq!(record["status"], "succeeded");
            assert!(record["startedAt"].as_str().unwrap().ends_with('Z'));
            assert!(record["endedAt"].as_str().unwrap().ends_with('Z'));
            assert_placeholder(record["command"].as_str().unwrap());
            assert_eq!(
                lines_of(&record),
                vec![
                    ("status", "running"),
                    ("text", "Hello"),
                    ("tool", "Read sources/notes.md"),
                    ("status", "succeeded"),
                ]
            );
            let missing = call("agent_job", json!({"id": "job-9"})).unwrap_err();
            assert!(missing.to_string().contains("no job job-9"), "{missing}");
            assert!(!root.join("jobs").exists());
        });
    }

    #[test]
    fn a_failing_scripted_job_ends_failed_with_the_error_line() {
        let (_tmp, root) = library();
        session(&root, Script::Fail, |call| {
            let error = call(
                "generate_outline",
                json!({"courseId": "lib", "topic": "Rust ownership", "sources": []}),
            )
            .unwrap_err();
            assert!(error.to_string().contains("quota exhausted"), "{error}");
            let record = call("agent_job", json!({"id": "job-1"})).unwrap();
            assert_eq!(record["status"], "failed");
            assert_placeholder(record["command"].as_str().unwrap());
            let lines = lines_of(&record);
            assert!(lines.contains(&("error", "quota exhausted")), "{lines:?}");
            assert_eq!(lines.last(), Some(&("status", "failed")));
        });
    }

    #[test]
    fn a_cancelled_scripted_job_ends_cancelled_without_an_error_line() {
        let (_tmp, root) = library();
        session(&root, Script::Cancel, |call| {
            let error = call(
                "generate_outline",
                json!({"courseId": "lib", "topic": "Rust ownership", "sources": []}),
            )
            .unwrap_err();
            assert!(error.to_string().contains("cancelled"), "{error}");
            let record = call("agent_job", json!({"id": "job-1"})).unwrap();
            assert_eq!(record["status"], "cancelled");
            assert_eq!(
                lines_of(&record),
                vec![("status", "running"), ("status", "cancelled")]
            );
        });
    }
}
