//! Generation commands: write a course outline, write a chapter when a
//! placeholder is opened, and write a chapter again on request.
//!
//! The engine does the work (`tutordocs_engine`); this module holds the one
//! `ChapterJobs` registry that keeps a chapter to one job, takes the driver
//! the user picked, and streams that job's progress to the webview over a
//! per-call channel.

use tauri::ipc::Channel;
use tauri::State;
use tutordocs_engine::{
    chapter_context as engine_chapter_context, job_scope, list_answers as engine_list_answers,
    ApproveGenerate, ChapterAnswer, ChapterContext, ChapterQueue, Progress,
};
pub use tutordocs_engine::{GenerationState, OutlineSummary};
use tutordocs_store::{Outline, OutlineVariant, VariantMeta};

use crate::jobs::JobRegistry;
use crate::reader::{ChapterDoc, ReaderState};

#[tauri::command]
pub async fn generate_outline(
    state: State<'_, GenerationState>,
    jobs: State<'_, JobRegistry>,
    course_id: String,
    topic: Option<String>,
    sources: Option<Vec<String>>,
) -> Result<OutlineSummary, String> {
    state
        .generate_outline(&jobs, &course_id, topic, sources)
        .await
}

#[tauri::command]
pub fn get_outline(state: State<'_, ReaderState>, course_id: String) -> Result<Outline, String> {
    state.with_course(&course_id, |library| {
        library.outline().map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn list_outline_variants(
    state: State<'_, ReaderState>,
    course_id: String,
) -> Result<Vec<VariantMeta>, String> {
    state.with_course(&course_id, |library| {
        library.list_outline_variants().map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn get_outline_variant(
    state: State<'_, ReaderState>,
    course_id: String,
    variant_id: String,
) -> Result<OutlineVariant, String> {
    state.with_course(&course_id, |library| {
        library
            .get_outline_variant(&variant_id)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn branch_outline(
    state: State<'_, ReaderState>,
    course_id: String,
    from_variant_id: String,
    label: String,
) -> Result<OutlineVariant, String> {
    state.with_course(&course_id, |library| {
        library
            .branch_outline(&from_variant_id, &label)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn set_current_variant(
    state: State<'_, GenerationState>,
    jobs: State<'_, JobRegistry>,
    course_id: String,
    variant_id: String,
) -> Result<OutlineVariant, String> {
    let running_jobs = jobs.list();
    for job in &running_jobs {
        if job.status == tutordocs_engine::JobStatus::Running
            && job.kind == tutordocs_engine::JobKind::Chapter
            && job.course_id == course_id
        {
            return Err(
                "cannot swap outline variant while a chapter job or queue is running".to_string(),
            );
        }
    }
    if let Ok(queue) = state.get_chapter_queue(&course_id) {
        if queue.running.is_some() || !queue.pending.is_empty() {
            return Err(
                "cannot swap outline variant while a chapter job or queue is running".to_string(),
            );
        }
    }
    state.set_current_variant(&course_id, &variant_id)
}

#[tauri::command]
pub fn save_outline(
    state: State<'_, ReaderState>,
    course_id: String,
    outline: Outline,
    variant_id: Option<String>,
) -> Result<Outline, String> {
    state.with_course(&course_id, |library| match variant_id {
        Some(vid) => library
            .save_outline_variant(&vid, &outline)
            .map_err(|e| e.to_string()),
        None => library.save_outline(&outline).map_err(|e| e.to_string()),
    })
}

#[tauri::command]
pub async fn revise_outline(
    state: State<'_, GenerationState>,
    jobs: State<'_, JobRegistry>,
    course_id: String,
    variant_id: Option<String>,
) -> Result<Outline, String> {
    state.revise_outline(&jobs, &course_id, variant_id).await
}

#[tauri::command]
pub async fn approve_outline(
    state: State<'_, GenerationState>,
    jobs: State<'_, JobRegistry>,
    course_id: String,
    generate: ApproveGenerate,
    variant_id: Option<String>,
) -> Result<Outline, String> {
    if variant_id.is_some() {
        let running_jobs = jobs.list();
        for job in &running_jobs {
            if job.status == tutordocs_engine::JobStatus::Running
                && job.kind == tutordocs_engine::JobKind::Chapter
                && job.course_id == course_id
            {
                return Err(
                    "cannot swap outline variant while a chapter job or queue is running"
                        .to_string(),
                );
            }
        }
        if let Ok(queue) = state.get_chapter_queue(&course_id) {
            if queue.running.is_some() || !queue.pending.is_empty() {
                return Err(
                    "cannot swap outline variant while a chapter job or queue is running"
                        .to_string(),
                );
            }
        }
    }
    let (outline, queued) = state
        .approve_outline_task(&course_id, generate, variant_id.as_deref())
        .await?;
    if let Some((task, driver)) = queued {
        let log = jobs.log();
        tauri::async_runtime::spawn(async move {
            job_scope(log, task.run(driver.as_ref())).await;
        });
    }
    Ok(outline)
}

#[tauri::command]
pub fn get_chapter_queue(
    state: State<'_, GenerationState>,
    course_id: String,
) -> Result<ChapterQueue, String> {
    state.get_chapter_queue(&course_id)
}

#[tauri::command]
pub fn cancel_chapter_queue(
    state: State<'_, GenerationState>,
    course_id: String,
) -> Result<(), String> {
    state.cancel_chapter_queue(&course_id)
}

#[tauri::command]
pub async fn open_chapter(
    state: State<'_, GenerationState>,
    jobs: State<'_, JobRegistry>,
    course_id: String,
    chapter_id: String,
    on_progress: Channel<Progress>,
) -> Result<ChapterDoc, String> {
    state
        .open_chapter(&jobs, &course_id, &chapter_id, &|p| {
            let _ = on_progress.send(p);
        })
        .await
}

#[tauri::command]
pub async fn regenerate_chapter(
    state: State<'_, GenerationState>,
    jobs: State<'_, JobRegistry>,
    course_id: String,
    chapter_id: String,
    instruction: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<ChapterDoc, String> {
    state
        .regenerate_chapter(
            &jobs,
            &course_id,
            &chapter_id,
            instruction.as_deref(),
            &|p| {
                let _ = on_progress.send(p);
            },
        )
        .await
}

#[tauri::command]
pub async fn revise_section(
    state: State<'_, GenerationState>,
    jobs: State<'_, JobRegistry>,
    course_id: String,
    chapter_id: String,
    quote: String,
    instruction: String,
) -> Result<ChapterDoc, String> {
    state
        .revise_section(&jobs, &course_id, &chapter_id, &quote, &instruction)
        .await
}

#[tauri::command]
pub async fn ask_about(
    state: State<'_, GenerationState>,
    jobs: State<'_, JobRegistry>,
    course_id: String,
    chapter_id: String,
    quote: Option<String>,
    question: String,
) -> Result<ChapterAnswer, String> {
    state
        .ask_about(&jobs, &course_id, &chapter_id, quote.as_deref(), &question)
        .await
}

#[tauri::command]
pub fn list_answers(
    state: State<'_, crate::reader::ReaderState>,
    course_id: String,
    chapter_id: String,
) -> Result<Vec<ChapterAnswer>, String> {
    state.with_course(&course_id, |library| {
        engine_list_answers(library.root(), &chapter_id).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn chapter_context(
    state: State<'_, crate::reader::ReaderState>,
    course_id: String,
    chapter_id: String,
) -> Result<ChapterContext, String> {
    state.with_course(&course_id, |library| {
        engine_chapter_context(library.root(), &chapter_id).map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::{install, ReaderState};
    use serde_json::{json, Value};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tutordocs_agent::AgentDriver;
    use tutordocs_store::{ChapterRef, Course, Library};

    /// The wire shapes both runtimes agree on; vitest replays the same file.
    fn contract() -> Value {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ui/src/fixtures/generate-contract.json"
        ));
        serde_json::from_str(text).expect("contract fixture is valid JSON")
    }

    /// A fake `claude`: an outline job answers with the contract's outline, a
    /// chapter job writes the contract's markdown into CHAPTER_DIR.
    const FAKE: &str = r#"#!/bin/sh
case "$1" in
  --version) echo "2.1.319 (Claude Code)"; exit 0 ;;
  --help) printf '%s\n' "-p, --print" "--output-format stream-json" "--json-schema" "--resume" "--append-system-prompt" "--tools" "--allowedTools" "--permission-mode"; exit 0 ;;
esac
echo job >> "$(dirname "$0")/jobs.log"
printf '%s' "$2" > "$(dirname "$0")/last-prompt.txt"
DIR=$(printf '%s\n' "$2" | sed -n 's/^CHAPTER_DIR: //p' | head -n 1)
echo '{"type":"system","subtype":"init","session_id":"s"}'
if [ -z "$DIR" ]; then
  echo '{"type":"result","subtype":"success","is_error":false,"result":"","structured_output":{"title":"Rust ownership","objectives":["Explain moves"],"chapters":[{"id":"moves","title":"Moves","summary":"Moves"},{"id":"borrows","title":"Borrows","summary":"Borrows"}]}}'
  exit 0
fi
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Grep","input":{}}]}}'
mkdir -p "$DIR"
printf '# Moves\n\nA move transfers ownership.\n' > "$DIR/index.md"
echo '{"type":"result","subtype":"success","is_error":false,"result":"Wrote the chapter."}'
"#;

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        claude: PathBuf,
        agents: crate::agents::AgentsState,
    }

    impl Fixture {
        fn jobs(&self) -> usize {
            let log = self.claude.with_file_name("jobs.log");
            std::fs::read_to_string(log).map_or(0, |text| text.lines().count())
        }

        /// The prompt the fake agent received on its latest job.
        fn last_prompt(&self) -> String {
            std::fs::read_to_string(self.claude.with_file_name("last-prompt.txt")).unwrap()
        }
    }

    /// A library named `lib` with an outline of two placeholder chapters.
    fn outlined() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("lib");
        let library = Library::create(&root).unwrap();
        let chapter = |id: &str, title: &str| ChapterRef {
            id: id.to_string(),
            title: title.to_string(),
            summary: String::new(),
            ..ChapterRef::default()
        };
        library
            .write_course(&Course {
                id: "lib".to_string(),
                title: "Rust ownership".to_string(),
                objectives: vec!["Explain moves".to_string()],
                chapters: vec![chapter("moves", "Moves"), chapter("borrows", "Borrows")],
                ..Course::default()
            })
            .unwrap();
        let claude = tmp.path().join("claude");
        std::fs::write(&claude, FAKE).unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        let agents = crate::agents::tests::claude_agents(&root, &claude);
        Fixture {
            _tmp: tmp,
            root,
            claude,
            agents,
        }
    }

    /// Run one command through the Tauri IPC layer, as the webview's `invoke` does.
    fn invoke(
        root: &Path,
        agents: &crate::agents::AgentsState,
        cmd: &str,
        args: Value,
    ) -> Result<Value, Value> {
        let app = install(mock_builder(), ReaderState::open(root))
            .manage(GenerationState::new(root.to_path_buf(), agents.clone()))
            .manage(agents.clone())
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds");
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
        .map(|body| body.deserialize::<Value>().expect("response is JSON"))
    }

    fn chapter_args(chapter_id: &str) -> Value {
        json!({"courseId": "lib", "chapterId": chapter_id, "onProgress": "__CHANNEL__:7"})
    }

    #[test]
    fn opening_a_placeholder_writes_it_and_returns_the_ready_chapter() {
        let fx = outlined();

        let response = invoke(&fx.root, &fx.agents, "open_chapter", chapter_args("moves"));

        assert_eq!(response.unwrap(), contract()["open_chapter_ready"]);
        assert_eq!(fx.jobs(), 1);
        let read = invoke(
            &fx.root,
            &fx.agents,
            "read_chapter",
            json!({"courseId": "lib", "chapterId": "moves"}),
        )
        .unwrap();
        assert_eq!(read, contract()["open_chapter_ready"]);
    }

    #[test]
    fn opening_a_written_chapter_runs_no_job() {
        let fx = outlined();
        invoke(&fx.root, &fx.agents, "open_chapter", chapter_args("moves")).unwrap();

        let again = invoke(&fx.root, &fx.agents, "open_chapter", chapter_args("moves"));

        assert_eq!(again.unwrap(), contract()["open_chapter_ready"]);
        assert_eq!(fx.jobs(), 1);
    }

    #[test]
    fn regenerating_runs_a_new_job_over_a_written_chapter() {
        let fx = outlined();
        std::fs::write(fx.root.join("chapters/moves/index.md"), "# Old\n").unwrap();

        let response = invoke(
            &fx.root,
            &fx.agents,
            "regenerate_chapter",
            chapter_args("moves"),
        );

        assert_eq!(response.unwrap(), contract()["open_chapter_ready"]);
        assert_eq!(fx.jobs(), 1);
    }

    fn rewrite_with(instruction: Option<Value>) -> String {
        let fx = outlined();
        std::fs::write(fx.root.join("chapters/moves/index.md"), "# Old\n").unwrap();
        let mut args = chapter_args("moves");
        if let Some(instruction) = instruction {
            args["instruction"] = instruction;
        }

        invoke(&fx.root, &fx.agents, "regenerate_chapter", args).unwrap();

        fx.last_prompt()
    }

    /// The learner's request text the fake agent was sent, if any.
    fn sent_request(prompt: &str) -> Option<String> {
        let mut lines = prompt.lines().skip_while(|l| *l != "BEGIN LEARNER REQUEST");
        lines.next()?;
        Some(serde_json::from_str(lines.next()?).expect("the request is a JSON string"))
    }

    #[test]
    fn regenerating_sends_the_learners_instruction_to_the_agent() {
        let prompt = rewrite_with(Some(json!("Explain it more simply.")));

        assert_eq!(
            sent_request(&prompt).as_deref(),
            Some("Explain it more simply.")
        );
    }

    #[test]
    fn regenerating_with_no_instruction_or_a_blank_one_sends_no_request() {
        for instruction in [None, Some(Value::Null), Some(json!("  \n "))] {
            let prompt = rewrite_with(instruction);

            assert_eq!(sent_request(&prompt), None);
            assert!(!prompt.contains("LEARNER REQUEST"));
        }
    }

    #[test]
    fn regenerating_caps_an_over_long_instruction_at_the_command_boundary() {
        let long = "z".repeat(tutordocs_engine::MAX_INSTRUCTION_CHARS + 4000);

        let prompt = rewrite_with(Some(json!(long)));

        let sent = sent_request(&prompt).expect("a request block");
        assert_eq!(
            sent.chars().count(),
            tutordocs_engine::MAX_INSTRUCTION_CHARS
        );
        assert!(
            prompt.len() < 6000,
            "the prompt stays small: {}",
            prompt.len()
        );
    }

    #[test]
    fn a_topic_becomes_an_outline_and_the_sidebar_lists_its_placeholders() {
        let fx = outlined();

        let response = invoke(
            &fx.root,
            &fx.agents,
            "generate_outline",
            json!({"courseId": "lib", "topic": "Rust ownership", "sources": []}),
        );

        assert_eq!(response.unwrap(), contract()["generate_outline"]);
        let listed = invoke(
            &fx.root,
            &fx.agents,
            "list_chapters",
            json!({"courseId": "lib"}),
        )
        .unwrap();
        let placeholders: Vec<bool> = listed
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["placeholder"].as_bool().unwrap())
            .collect();
        assert_eq!(placeholders, vec![true, true]);
    }

    #[test]
    fn with_no_agent_a_job_reports_what_to_install_and_writes_nothing() {
        let fx = outlined();

        let none = crate::agents::tests::no_agents(&fx.root);
        let error = invoke(&fx.root, &none, "open_chapter", chapter_args("moves")).unwrap_err();

        assert!(error.to_string().contains("No agent CLI"), "{error}");
        assert!(!fx.root.join("chapters/moves/index.md").exists());
    }

    #[test]
    fn unknown_courses_and_chapters_are_refused_before_any_job() {
        let fx = outlined();
        let missing_course = json!({"topic": "Rust ownership", "sources": []});
        assert!(invoke(&fx.root, &fx.agents, "generate_outline", missing_course).is_err());
        let wrong_course =
            json!({"courseId": "x", "chapterId": "moves", "onProgress": "__CHANNEL__:7"});
        assert!(invoke(&fx.root, &fx.agents, "open_chapter", wrong_course).is_err());
        let wrong_chapter = chapter_args("../moves");
        assert!(invoke(&fx.root, &fx.agents, "open_chapter", wrong_chapter).is_err());
        assert_eq!(fx.jobs(), 0);
    }

    /// Records the job a command handed it, then stops. Nothing is spawned.
    struct Recorder {
        id: &'static str,
        hits: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait::async_trait]
    impl tutordocs_agent::AgentDriver for Recorder {
        fn id(&self) -> &'static str {
            self.id
        }
        fn capabilities(&self) -> tutordocs_agent::Capabilities {
            tutordocs_agent::Capabilities {
                headless: true,
                stream_json: true,
                structured_output: true,
                resume: true,
                system_append: true,
                tool_allowlist: true,
                read_only: true,
                model: false,
                effort: false,
                auth: tutordocs_agent::AuthKind::SubscriptionOAuth,
            }
        }
        async fn start(
            &self,
            job: tutordocs_agent::JobSpec,
        ) -> anyhow::Result<tutordocs_agent::JobHandle> {
            let kind = if job.allowed_tools.is_some()
                && job.prompt.contains("https://example.com/picked")
            {
                "url"
            } else if !job.read_only && job.prompt.contains("CHAPTER_DIR:") {
                "chapter"
            } else if job.read_only
                && job.json_schema.is_some()
                && job.prompt.contains("TOPIC: Rust ownership")
            {
                "outline"
            } else {
                "other"
            };
            self.hits.lock().expect("hits").push(kind);
            anyhow::bail!("recorded")
        }
        async fn resume(
            &self,
            _: &str,
            _: tutordocs_agent::JobSpec,
        ) -> anyhow::Result<tutordocs_agent::JobHandle> {
            anyhow::bail!("recorded")
        }
        fn events(&self, handle: &tutordocs_agent::JobHandle) -> tutordocs_agent::EventStream {
            tutordocs_agent::EventStream::for_job(handle)
        }
        async fn cancel(&self, _: &tutordocs_agent::JobHandle) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn detected(
        id: &str,
        driver: Option<Arc<dyn tutordocs_agent::AgentDriver>>,
    ) -> tutordocs_agent::registry::Detected {
        use tutordocs_agent::registry::{Detected, DriverStatus};
        Detected {
            status: DriverStatus {
                id: id.to_string(),
                available: driver.is_some(),
                binary: None,
                version: None,
                capabilities: driver.as_ref().map(|d| d.capabilities()),
                reason: None,
            },
            driver,
        }
    }

    /// Outline, chapter and URL commands all run on the driver the picker saved,
    /// including when that driver is not Claude.
    #[test]
    fn a_non_claude_selection_reaches_outline_chapter_and_url_jobs() {
        use crate::agents::AgentsState;

        let fx = outlined();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let qoder = Arc::new(Recorder {
            id: "qoder",
            hits: hits.clone(),
        });
        let claude_hits = Arc::new(Mutex::new(Vec::new()));
        let claude = Arc::new(Recorder {
            id: "claude",
            hits: claude_hits.clone(),
        });
        // The saved choice is qoder. Claude is also available, so a command that
        // ignores the choice and builds its own Claude driver fails this test.
        std::fs::create_dir_all(fx.root.join(".tutordocs")).unwrap();
        std::fs::write(
            fx.root.join(".tutordocs/agent.json"),
            r#"{"driver":"qoder"}"#,
        )
        .unwrap();
        let qoder_driver = qoder.clone();
        let claude_driver = claude.clone();
        let agents = AgentsState::new(
            &fx.root,
            Box::new(move || {
                vec![
                    detected("claude", Some(claude_driver.clone())),
                    detected("qoder", Some(qoder_driver.clone())),
                    detected("agy", None),
                ]
            }),
        );
        let run = |cmd: &str, args: Value| {
            let app = install(mock_builder(), ReaderState::open(&fx.root))
                .manage(GenerationState::new(fx.root.clone(), agents.clone()))
                .manage(agents.clone())
                .build(mock_context(noop_assets()))
                .expect("mock app builds");
            let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
                .build()
                .expect("mock webview builds");
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
        };

        let _ = run(
            "generate_outline",
            json!({"courseId": "lib", "topic": "Rust ownership", "sources": []}),
        );
        let _ = run("open_chapter", chapter_args("moves"));
        let _ = run("ingest_url", json!({"url": "https://example.com/picked"}));

        assert_eq!(
            hits.lock().unwrap().as_slice(),
            ["outline", "chapter", "url"]
        );
        assert!(claude_hits.lock().unwrap().is_empty());
    }

    /// The OS sandbox is what confines agy, so an outline job is no longer
    /// refused before launch. The fake child records the prompt and exits
    /// without a result. The command's error must not quote the binary path.
    #[test]
    fn a_read_only_job_on_agy_runs_and_the_error_quotes_no_binary() {
        let fx = outlined();
        let log = fx.claude.with_file_name("agy-prompts.log");
        let binary = fx.claude.with_file_name("agy");
        let script = format!(
            "#!/bin/sh\ncase \"$1\" in\n  --version) echo 1.0.0; exit 0 ;;\n  --help) printf '%s\\n' --print stream-json; exit 0 ;;\nesac\necho \"$*\" >> '{}'\n",
            log.display()
        );
        std::fs::write(&binary, script).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let sandbox = fx.claude.with_file_name("sandbox-exec");
        std::fs::write(&sandbox, "#!/bin/sh\nshift 2\nexec \"$@\"\n").unwrap();
        std::fs::set_permissions(&sandbox, std::fs::Permissions::from_mode(0o755)).unwrap();
        let driver = tutordocs_agent::AgyDriver::new(&binary)
            .expect("fake agy probes")
            .with_sandbox_exec(&sandbox);
        assert!(driver.capabilities().read_only);
        assert!(driver.capabilities().tool_allowlist);
        let driver: Arc<dyn tutordocs_agent::AgentDriver> = Arc::new(driver);
        std::fs::write(fx.root.join(".tutordocs/agent.json"), r#"{"driver":"agy"}"#).unwrap();
        let agents = crate::agents::AgentsState::new(
            &fx.root,
            Box::new(move || vec![detected("agy", Some(driver.clone()))]),
        );

        let error = invoke(
            &fx.root,
            &agents,
            "generate_outline",
            json!({"courseId": "lib", "topic": "Rust ownership", "sources": []}),
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(!message.contains("refusing a read-only job"), "{message}");
        assert!(
            !message.contains(binary.to_str().unwrap()),
            "the reason must not quote the binary path: {message}"
        );
        assert!(log.exists(), "the outline prompt must reach the child");
    }

    /// Outline, chapter, rewrite, URL ingest, quiz generation and grading all
    /// receive the model and effort picked for the driver in use.
    #[test]
    fn a_picked_model_and_effort_reach_every_job() {
        let fx = outlined();
        let jobs = Arc::new(Mutex::new(Vec::new()));
        let record = jobs.clone();
        let recorder = Arc::new(SelectionRecorder { jobs: record });
        let agents = crate::agents::AgentsState::new(
            &fx.root,
            Box::new(move || {
                vec![
                    detected("claude", Some(recorder.clone())),
                    detected("qoder", None),
                    detected("agy", None),
                ]
            }),
        );
        let set_model = invoke(
            &fx.root,
            &agents,
            "set_driver_model",
            json!({"id": "claude", "model": "opus"}),
        )
        .unwrap();
        assert_eq!(set_model["model"], "opus");
        let set_effort = invoke(
            &fx.root,
            &agents,
            "set_driver_effort",
            json!({"id": "claude", "effort": "xhigh"}),
        )
        .unwrap();
        assert_eq!(set_effort["effort"], "xhigh");

        let _ = invoke(
            &fx.root,
            &agents,
            "generate_outline",
            json!({"courseId": "lib", "topic": "Rust ownership", "sources": []}),
        );
        let _ = invoke(&fx.root, &agents, "open_chapter", chapter_args("moves"));
        let course = tutordocs_store::Shelf::open(&fx.root)
            .unwrap()
            .active_course_dir()
            .unwrap();
        std::fs::write(
            course.join("chapters/moves/index.md"),
            "# Moves\n\nA move.\n",
        )
        .unwrap();
        let _ = invoke(
            &fx.root,
            &agents,
            "regenerate_chapter",
            chapter_args("moves"),
        );
        let _ = invoke(
            &fx.root,
            &agents,
            "ingest_url",
            json!({"url": "https://example.com/picked"}),
        );
        let _ = invoke(
            &fx.root,
            &agents,
            "generate_quiz",
            json!({"chapterId": "moves"}),
        );
        let _ = invoke(
            &fx.root,
            &agents,
            "grade_quiz",
            json!({
                "chapterId": "moves",
                "answers": [{
                    "question": {
                        "id": "q1",
                        "objective": "Explain moves",
                        "prompt": "Why does the later use fail?",
                        "referenceAnswer": "Because a move transfers ownership.",
                        "combines": []
                    },
                    "answer": "I am not sure."
                }]
            }),
        );

        let seen = jobs.lock().expect("jobs");
        assert_eq!(seen.len(), 6, "one start per job path, saw {seen:?}");
        assert!(
            seen.iter()
                .all(|pick| pick == &("opus".to_string(), "xhigh".to_string())),
            "{seen:?}"
        );
    }

    /// Records the model and effort a command handed the driver, then stops.
    struct SelectionRecorder {
        jobs: Arc<Mutex<Vec<(String, String)>>>,
    }

    #[async_trait::async_trait]
    impl tutordocs_agent::AgentDriver for SelectionRecorder {
        fn id(&self) -> &'static str {
            "claude"
        }
        fn capabilities(&self) -> tutordocs_agent::Capabilities {
            tutordocs_agent::Capabilities {
                headless: true,
                stream_json: true,
                structured_output: true,
                resume: true,
                system_append: true,
                tool_allowlist: true,
                read_only: true,
                model: true,
                effort: true,
                auth: tutordocs_agent::AuthKind::SubscriptionOAuth,
            }
        }
        async fn start(
            &self,
            job: tutordocs_agent::JobSpec,
        ) -> anyhow::Result<tutordocs_agent::JobHandle> {
            let model = job
                .model
                .map(|model| model.as_str().to_string())
                .unwrap_or_default();
            let effort = job
                .effort
                .map(|effort| effort.as_str().to_string())
                .unwrap_or_default();
            self.jobs.lock().expect("jobs").push((model, effort));
            anyhow::bail!("recorded")
        }
        async fn resume(
            &self,
            _: &str,
            _: tutordocs_agent::JobSpec,
        ) -> anyhow::Result<tutordocs_agent::JobHandle> {
            anyhow::bail!("recorded")
        }
        fn events(&self, handle: &tutordocs_agent::JobHandle) -> tutordocs_agent::EventStream {
            tutordocs_agent::EventStream::for_job(handle)
        }
        async fn cancel(&self, _: &tutordocs_agent::JobHandle) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn progress_crosses_the_channel_in_the_contract_shape() {
        let events = [
            Progress::Started,
            Progress::Tool {
                name: "Grep".to_string(),
            },
            Progress::Text { chars: 18 },
        ];
        let wire = serde_json::to_value(events).unwrap();
        assert_eq!(wire, contract()["progress"]);
    }

    #[test]
    fn generate_outline_writes_a_draft_and_open_chapter_refuses_until_approved() {
        let fx = outlined();

        let summary = invoke(
            &fx.root,
            &fx.agents,
            "generate_outline",
            json!({"courseId": "lib"}),
        )
        .unwrap();
        assert_eq!(summary, contract()["generate_outline"]);
        assert_eq!(fx.jobs(), 1);

        let outline = invoke(
            &fx.root,
            &fx.agents,
            "get_outline",
            json!({"courseId": "lib"}),
        )
        .unwrap();
        assert_eq!(outline["status"], "draft");
        assert_eq!(outline["chapters"].as_array().unwrap().len(), 2);

        let err = invoke(&fx.root, &fx.agents, "open_chapter", chapter_args("moves")).unwrap_err();
        assert!(
            err.to_string().contains("outline is a draft"),
            "expected draft refusal, got {err}"
        );
        assert_eq!(fx.jobs(), 1, "refused before spawning a chapter job");

        let approved = invoke(
            &fx.root,
            &fx.agents,
            "approve_outline",
            json!({"courseId": "lib", "generate": "on_open"}),
        )
        .unwrap();
        assert_eq!(approved["status"], "approved");

        let doc = invoke(&fx.root, &fx.agents, "open_chapter", chapter_args("moves")).unwrap();
        assert_eq!(doc, contract()["open_chapter_ready"]);
        assert_eq!(fx.jobs(), 2);
    }

    #[test]
    fn save_outline_edits_and_refuses_removing_written_or_unknown_chapters_over_ipc() {
        let fx = outlined();
        std::fs::write(fx.root.join("chapters/moves/index.md"), "# Moves\n").unwrap();

        let initial = invoke(
            &fx.root,
            &fx.agents,
            "get_outline",
            json!({"courseId": "lib"}),
        )
        .unwrap();
        assert_eq!(initial["status"], "approved");
        assert_eq!(initial["chapters"][0]["written"], true);
        assert_eq!(initial["chapters"][1]["written"], false);

        let drop_written = invoke(
            &fx.root,
            &fx.agents,
            "save_outline",
            json!({
                "courseId": "lib",
                "outline": {
                    "courseId": "lib",
                    "title": "Rust ownership",
                    "status": "draft",
                    "objectives": ["Explain moves"],
                    "chapters": [{
                        "id": "borrows",
                        "title": "Borrows",
                        "summary": "",
                        "sources": [],
                        "written": false,
                        "comment": ""
                    }]
                }
            }),
        );
        assert!(drop_written.is_err());

        let unknown_id = invoke(
            &fx.root,
            &fx.agents,
            "save_outline",
            json!({
                "courseId": "lib",
                "outline": {
                    "courseId": "lib",
                    "title": "Rust ownership",
                    "status": "draft",
                    "objectives": ["Explain moves"],
                    "chapters": [
                        {
                            "id": "moves",
                            "title": "Moves",
                            "summary": "",
                            "sources": [],
                            "written": true,
                            "comment": ""
                        },
                        {
                            "id": "ghost",
                            "title": "Ghost",
                            "summary": "",
                            "sources": [],
                            "written": false,
                            "comment": ""
                        }
                    ]
                }
            }),
        );
        assert!(unknown_id.is_err());

        let edited_payload = json!({
            "courseId": "lib",
            "title": "Ownership & Lifetimes",
            "status": "draft",
            "objectives": ["Explain moves", "Use lifetimes"],
            "chapters": [
                {
                    "id": null,
                    "title": "Lifetimes & Scopes",
                    "summary": "Named lifetimes",
                    "sources": ["notes.md"],
                    "written": false,
                    "comment": "Split examples"
                },
                {
                    "id": "moves",
                    "title": "Move Semantics",
                    "summary": "Ownership transfer",
                    "sources": [],
                    "written": false,
                    "comment": ""
                }
            ]
        });
        let saved = invoke(
            &fx.root,
            &fx.agents,
            "save_outline",
            json!({"courseId": "lib", "outline": edited_payload}),
        )
        .unwrap();
        assert_eq!(saved["title"], "Ownership & Lifetimes");
        assert_eq!(saved["chapters"][0]["id"], "lifetimes-scopes");
        assert_eq!(saved["chapters"][0]["comment"], "Split examples");
        assert_eq!(saved["chapters"][1]["id"], "moves");
        assert_eq!(saved["chapters"][1]["written"], true);

        let saved_twice = invoke(
            &fx.root,
            &fx.agents,
            "save_outline",
            json!({"courseId": "lib", "outline": saved}),
        )
        .unwrap();
        assert_eq!(saved_twice, saved);
    }

    #[test]
    fn revise_outline_keeps_written_chapters_and_clears_comments_over_ipc() {
        let fx = outlined();
        invoke(
            &fx.root,
            &fx.agents,
            "generate_outline",
            json!({"courseId": "lib"}),
        )
        .unwrap();
        let course_dir = tutordocs_store::Shelf::open(&fx.root)
            .unwrap()
            .active_course_dir()
            .unwrap();
        std::fs::write(course_dir.join("chapters/moves/index.md"), "# Moves\n").unwrap();
        let edited = json!({
            "courseId": "lib",
            "title": "Rust ownership",
            "status": "draft",
            "objectives": ["Explain moves"],
            "chapters": [
                {
                    "id": "moves",
                    "title": "Moves",
                    "summary": "Old summary",
                    "sources": [],
                    "written": true,
                    "comment": "Keep moves"
                },
                {
                    "id": "borrows",
                    "title": "Borrows",
                    "summary": "Old borrows",
                    "sources": [],
                    "written": false,
                    "comment": "Expand borrows"
                }
            ]
        });
        invoke(
            &fx.root,
            &fx.agents,
            "save_outline",
            json!({"courseId": "lib", "outline": edited}),
        )
        .unwrap();

        let revised = invoke(
            &fx.root,
            &fx.agents,
            "revise_outline",
            json!({"courseId": "lib"}),
        )
        .unwrap();
        assert_eq!(revised["status"], "draft");
        assert_eq!(revised["chapters"][0]["id"], "moves");
        assert_eq!(revised["chapters"][0]["written"], true);
        assert_eq!(revised["chapters"][0]["comment"], "");
        assert_eq!(revised["chapters"][1]["id"], "borrows");
        assert_eq!(revised["chapters"][1]["comment"], "");
    }

    #[test]
    fn approve_outline_all_queues_unwritten_chapters_and_cancel_stops_the_queue_over_ipc() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Condvar;
        use tutordocs_agent::{
            AgentDriver, AgentEvent, AuthKind, Capabilities, EventStream, JobHandle, JobSpec,
        };

        struct Gate {
            ready: Mutex<bool>,
            cvar: Condvar,
        }

        impl Gate {
            fn new() -> Arc<Self> {
                Arc::new(Self {
                    ready: Mutex::new(false),
                    cvar: Condvar::new(),
                })
            }

            fn signal(&self) {
                *self.ready.lock().unwrap() = true;
                self.cvar.notify_all();
            }

            fn wait(&self) {
                let mut guard = self.ready.lock().unwrap();
                while !*guard {
                    guard = self.cvar.wait(guard).unwrap();
                }
            }
        }

        struct SlowDriver {
            started: Arc<AtomicUsize>,
            first_started: Arc<Gate>,
            release_first: Arc<Gate>,
            done_gate: Arc<Gate>,
        }

        #[async_trait::async_trait]
        impl AgentDriver for SlowDriver {
            fn id(&self) -> &'static str {
                "claude"
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
            async fn start(&self, job: JobSpec) -> anyhow::Result<JobHandle> {
                let n = self.started.fetch_add(1, Ordering::SeqCst);
                let rel = job
                    .prompt
                    .lines()
                    .find_map(|line| line.strip_prefix("CHAPTER_DIR: "))
                    .unwrap()
                    .trim()
                    .to_string();
                let dir = job.workdir.join(&rel);
                if n == 0 {
                    self.first_started.signal();
                    self.release_first.wait();
                }
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join("index.md"), format!("# {rel}\n"))?;
                self.done_gate.signal();
                Ok(JobHandle::scripted_with_command(
                    vec!["claude".into(), "-p".into(), job.prompt],
                    vec![
                        AgentEvent::Result {
                            value: json!("Wrote the chapter."),
                        },
                        AgentEvent::Done,
                    ],
                ))
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

        let fx = outlined();
        let started = Arc::new(AtomicUsize::new(0));
        let first_started = Gate::new();
        let release_first = Gate::new();
        let done_gate = Gate::new();
        let driver: Arc<dyn AgentDriver> = Arc::new(SlowDriver {
            started: started.clone(),
            first_started: first_started.clone(),
            release_first: release_first.clone(),
            done_gate: done_gate.clone(),
        });
        let agents = crate::agents::AgentsState::new(
            &fx.root,
            Box::new(move || vec![detected("claude", Some(driver.clone()))]),
        );
        let app = install(mock_builder(), ReaderState::open(&fx.root))
            .manage(GenerationState::new(fx.root.clone(), agents.clone()))
            .manage(agents)
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
            .map(|body| body.deserialize::<Value>().expect("response is JSON"))
        };

        // Branch a variant before approving queue
        let draft_b = call(
            "branch_outline",
            json!({"courseId": "lib", "fromVariantId": "current", "label": "Draft B"}),
        )
        .unwrap();
        assert_eq!(draft_b["id"], "draft-b");

        let approved = call(
            "approve_outline",
            json!({"courseId": "lib", "generate": "all"}),
        )
        .unwrap();
        assert_eq!(approved["status"], "approved");

        // Second approve_outline while first queue is active queues nothing new.
        let approved_again = call(
            "approve_outline",
            json!({"courseId": "lib", "generate": "all"}),
        )
        .unwrap();
        assert_eq!(approved_again["status"], "approved");

        // Wait for the first queued chapter job to start, then cancel the queue.
        first_started.wait();
        assert_eq!(
            started.load(Ordering::SeqCst),
            1,
            "runs one chapter job at a time"
        );
        assert_eq!(
            call("get_chapter_queue", json!({"courseId": "lib"})).unwrap(),
            json!({"running": "moves", "pending": ["borrows"]})
        );

        // While queue is active, set_current_variant and approve_outline with variantId are refused
        let swap_err = call(
            "set_current_variant",
            json!({"courseId": "lib", "variantId": "draft-b"}),
        )
        .unwrap_err();
        assert!(
            swap_err
                .to_string()
                .contains("cannot swap outline variant while a chapter job or queue is running"),
            "{swap_err}"
        );
        let approve_swap_err = call(
            "approve_outline",
            json!({"courseId": "lib", "generate": "on_open", "variantId": "draft-b"}),
        )
        .unwrap_err();
        assert!(
            approve_swap_err
                .to_string()
                .contains("cannot swap outline variant while a chapter job or queue is running"),
            "{approve_swap_err}"
        );

        call("cancel_chapter_queue", json!({"courseId": "lib"})).unwrap();
        assert_eq!(
            call("get_chapter_queue", json!({"courseId": "lib"})).unwrap(),
            json!({"running": "moves", "pending": []})
        );
        release_first.signal();
        done_gate.wait();
        // Wait for job-1 in JobRegistry to transition from running to succeeded
        // and the queue to drain.
        for _ in 0..100 {
            let jobs = call("list_agent_jobs", json!({})).unwrap();
            let queue = call("get_chapter_queue", json!({"courseId": "lib"})).unwrap();
            if jobs[0]["status"] == "succeeded" && queue["running"].is_null() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            call("get_chapter_queue", json!({"courseId": "lib"})).unwrap(),
            json!({"running": null, "pending": []})
        );

        let course_dir = tutordocs_store::Shelf::open(&fx.root)
            .unwrap()
            .active_course_dir()
            .unwrap();
        assert_eq!(started.load(Ordering::SeqCst), 1, "stopped after first job");
        assert!(course_dir.join("chapters/moves/index.md").is_file());
        assert!(!course_dir.join("chapters/borrows/index.md").is_file());

        let jobs = call("list_agent_jobs", json!({})).unwrap();
        let jobs = jobs.as_array().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["kind"], "chapter");

        // Re-approving with "all" queues the remaining unwritten chapter ("borrows")
        // and drains to completion.
        call(
            "approve_outline",
            json!({"courseId": "lib", "generate": "all"}),
        )
        .unwrap();
        for _ in 0..100 {
            let jobs = call("list_agent_jobs", json!({})).unwrap();
            let queue = call("get_chapter_queue", json!({"courseId": "lib"})).unwrap();
            let arr = jobs.as_array().unwrap();
            if arr.len() == 2 && arr[0]["status"] == "succeeded" && queue["running"].is_null() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            call("get_chapter_queue", json!({"courseId": "lib"})).unwrap(),
            json!({"running": null, "pending": []})
        );
        assert!(course_dir.join("chapters/borrows/index.md").is_file());
    }

    struct SectionAndAskDriver;

    #[async_trait::async_trait]
    impl tutordocs_agent::AgentDriver for SectionAndAskDriver {
        fn id(&self) -> &'static str {
            "claude"
        }
        fn capabilities(&self) -> tutordocs_agent::Capabilities {
            tutordocs_agent::Capabilities {
                headless: true,
                stream_json: true,
                structured_output: true,
                resume: true,
                system_append: true,
                tool_allowlist: true,
                read_only: true,
                model: true,
                effort: true,
                auth: tutordocs_agent::AuthKind::SubscriptionOAuth,
            }
        }
        async fn start(
            &self,
            job: tutordocs_agent::JobSpec,
        ) -> anyhow::Result<tutordocs_agent::JobHandle> {
            use tutordocs_agent::{AgentEvent, JobHandle};
            let cmd = vec!["claude".into(), "-p".into(), job.prompt.clone()];
            if job.read_only {
                let answer = json!({
                    "answer": "Ownership moves to the new binding.",
                    "sources": ["sources/lecture.pdf"]
                });
                Ok(JobHandle::scripted_with_command(
                    cmd,
                    vec![AgentEvent::Result { value: answer }, AgentEvent::Done],
                ))
            } else {
                let out = job.workdir.join("chapter");
                std::fs::create_dir_all(&out).unwrap();
                std::fs::write(
                    out.join("index.md"),
                    "## Heap moves\n\nRewritten heap section.\n",
                )
                .unwrap();
                Ok(JobHandle::scripted_with_command(
                    cmd,
                    vec![
                        AgentEvent::Result {
                            value: json!("Revised section."),
                        },
                        AgentEvent::Done,
                    ],
                ))
            }
        }
        async fn resume(
            &self,
            _: &str,
            job: tutordocs_agent::JobSpec,
        ) -> anyhow::Result<tutordocs_agent::JobHandle> {
            self.start(job).await
        }
        fn events(&self, handle: &tutordocs_agent::JobHandle) -> tutordocs_agent::EventStream {
            tutordocs_agent::EventStream::for_job(handle)
        }
        async fn cancel(&self, _: &tutordocs_agent::JobHandle) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn revise_section_ask_about_list_answers_and_chapter_context_work_through_ipc() {
        let fx = outlined();
        let course_json = json!({
            "id": "lib",
            "title": "Rust ownership",
            "objectives": ["Explain moves"],
            "chapters": [
                {
                    "id": "moves",
                    "title": "Moves",
                    "summary": "What a move does",
                    "sources": ["lecture.pdf"]
                },
                {
                    "id": "borrows",
                    "title": "Borrows",
                    "summary": "Borrows"
                }
            ]
        });
        std::fs::write(
            fx.root.join("course.json"),
            serde_json::to_string_pretty(&course_json).unwrap(),
        )
        .unwrap();
        std::fs::write(
            fx.root.join("chapters/moves/index.md"),
            "# Moves\n\nSee sources/notes.md and sources/lecture.pdf.\n\n## Heap moves\n\nOld heap text.\n",
        )
        .unwrap();

        let driver: Arc<dyn tutordocs_agent::AgentDriver> = Arc::new(SectionAndAskDriver);
        let agents = crate::agents::AgentsState::new(
            &fx.root,
            Box::new(move || vec![detected("claude", Some(driver.clone()))]),
        );
        let app = install(mock_builder(), ReaderState::open(&fx.root))
            .manage(GenerationState::new(fx.root.clone(), agents.clone()))
            .manage(agents)
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds");
        let call = |cmd: &str, args: Value| -> Result<Value, Value> {
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
            .map(|body| body.deserialize::<Value>().expect("response is JSON"))
        };

        let ctx = call(
            "chapter_context",
            json!({"courseId": "lib", "chapterId": "moves"}),
        )
        .unwrap();
        assert_eq!(
            ctx,
            json!({
                "objectives": ["Explain moves"],
                "summary": "What a move does",
                "sources": ["lecture.pdf", "notes.md"]
            })
        );

        let revised = call(
            "revise_section",
            json!({
                "courseId": "lib",
                "chapterId": "moves",
                "quote": "Old heap text.",
                "instruction": "Clarify heap moves"
            }),
        )
        .unwrap();
        assert_eq!(
            revised,
            json!({
                "id": "moves",
                "title": "Moves",
                "state": "ready",
                "markdown": "# Moves\n\nSee sources/notes.md and sources/lecture.pdf.\n\n## Heap moves\n\nRewritten heap section.\n"
            })
        );

        let a1 = call(
            "ask_about",
            json!({
                "courseId": "lib",
                "chapterId": "moves",
                "quote": "Rewritten heap section.",
                "question": "What happens to the heap pointer?"
            }),
        )
        .unwrap();
        assert_eq!(a1["id"], 1);
        assert_eq!(a1["quote"], "Rewritten heap section.");
        assert_eq!(a1["question"], "What happens to the heap pointer?");
        assert_eq!(a1["answer"], "Ownership moves to the new binding.");
        assert_eq!(a1["sources"], json!(["lecture.pdf"]));
        assert!(a1["askedAt"].as_str().unwrap().ends_with('Z'));

        let a2 = call(
            "ask_about",
            json!({
                "courseId": "lib",
                "chapterId": "moves",
                "quote": null,
                "question": "Summary question?"
            }),
        )
        .unwrap();
        assert_eq!(a2["id"], 2);
        assert!(a2["quote"].is_null());

        let listed = call(
            "list_answers",
            json!({"courseId": "lib", "chapterId": "moves"}),
        )
        .unwrap();
        assert_eq!(listed, json!([a2, a1]));

        let jobs = call("list_agent_jobs", json!({})).unwrap();
        let kinds: Vec<&str> = jobs
            .as_array()
            .unwrap()
            .iter()
            .map(|j| j["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, vec!["rewrite", "ask", "ask"]);
    }

    #[test]
    fn outline_variants_lifecycle_over_ipc() {
        let fx = outlined();

        // 1. Initial variants list has "current"
        let variants = invoke(
            &fx.root,
            &fx.agents,
            "list_outline_variants",
            json!({"courseId": "lib"}),
        )
        .unwrap();
        let arr = variants.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "current");
        assert_eq!(arr[0]["label"], "Current");

        // 2. get_outline_variant current
        let current_variant = invoke(
            &fx.root,
            &fx.agents,
            "get_outline_variant",
            json!({"courseId": "lib", "variantId": "current"}),
        )
        .unwrap();
        assert_eq!(current_variant["id"], "current");
        assert_eq!(current_variant["chapters"].as_array().unwrap().len(), 2);

        // 3. branch to Draft B
        let draft_b = invoke(
            &fx.root,
            &fx.agents,
            "branch_outline",
            json!({"courseId": "lib", "fromVariantId": "current", "label": "Draft B"}),
        )
        .unwrap();
        assert_eq!(draft_b["id"], "draft-b");
        assert_eq!(draft_b["label"], "Draft B");
        assert_eq!(draft_b["parentId"], "current");

        // 4. save_outline on draft-b
        let edited_draft_b = json!({
            "status": "draft",
            "title": "Rust ownership",
            "objectives": draft_b["objectives"],
            "chapters": [
                draft_b["chapters"][0].clone(),
                json!({
                    "id": "borrows",
                    "title": "Borrows and Aliasing",
                    "summary": "",
                    "sources": [],
                    "comment": "",
                    "written": false
                })
            ]
        });
        let saved_b = invoke(
            &fx.root,
            &fx.agents,
            "save_outline",
            json!({"courseId": "lib", "variantId": "draft-b", "outline": edited_draft_b}),
        )
        .unwrap();
        assert_eq!(saved_b["chapters"][1]["title"], "Borrows and Aliasing");

        // 5. current outline is still unchanged in course.json
        let curr_outline = invoke(
            &fx.root,
            &fx.agents,
            "get_outline",
            json!({"courseId": "lib"}),
        )
        .unwrap();
        assert_eq!(curr_outline["chapters"][1]["title"], "Borrows");

        // 6. swap to draft-b
        let swapped = invoke(
            &fx.root,
            &fx.agents,
            "set_current_variant",
            json!({"courseId": "lib", "variantId": "draft-b"}),
        )
        .unwrap();
        assert_eq!(swapped["id"], "draft-b");

        // Now get_outline reads draft-b
        let active = invoke(
            &fx.root,
            &fx.agents,
            "get_outline",
            json!({"courseId": "lib"}),
        )
        .unwrap();
        assert_eq!(active["chapters"][1]["title"], "Borrows and Aliasing");

        // 7. approve_outline promotes and sets approved
        let approved = invoke(
            &fx.root,
            &fx.agents,
            "approve_outline",
            json!({"courseId": "lib", "generate": "on_open", "variantId": "current"}),
        )
        .unwrap();
        assert_eq!(approved["status"], "approved");
        assert_eq!(approved["chapters"][1]["title"], "Borrows");
    }
}
