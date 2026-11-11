//! Agent driver commands: which coding-agent CLIs this machine has, which one
//! the user chose, and the model and effort remembered for each one.
//!
//! Detection (`tutordocs_agent::registry`) probes each CLI with `--version` and
//! `--help`, up to ten seconds each, so it runs on a blocking thread and its
//! result is cached. The choice is a small file inside the library, so it
//! survives a restart. Job-start commands take the driver from
//! [`AgentsState::driver`], which stamps that driver's model and effort onto
//! the job, so the picker and every job agree.

use tauri::State;
use tutordocs_agent::catalog::DriverCatalog;
pub use tutordocs_engine::{AgentChoice, AgentsState, DriverTuning};

/// Run `work` off the async executor and the UI thread.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("the agent check did not finish: {e}"))?
}

#[tauri::command]
pub async fn list_agent_drivers(state: State<'_, AgentsState>) -> Result<AgentChoice, String> {
    let state = state.inner().clone();
    blocking(move || Ok(state.list())).await
}

#[tauri::command]
pub async fn refresh_agent_drivers(state: State<'_, AgentsState>) -> Result<AgentChoice, String> {
    let state = state.inner().clone();
    blocking(move || Ok(state.refresh())).await
}

#[tauri::command]
pub async fn set_selected_driver(
    state: State<'_, AgentsState>,
    id: String,
) -> Result<AgentChoice, String> {
    let state = state.inner().clone();
    blocking(move || state.select(&id)).await
}

#[tauri::command]
pub async fn list_driver_models(
    state: State<'_, AgentsState>,
    id: String,
) -> Result<DriverCatalog, String> {
    let state = state.inner().clone();
    blocking(move || state.list_models(&id)).await
}

#[tauri::command]
pub async fn driver_tuning(
    state: State<'_, AgentsState>,
    id: String,
) -> Result<DriverTuning, String> {
    let state = state.inner().clone();
    blocking(move || state.tuning_of(&id)).await
}

#[tauri::command]
pub async fn set_driver_model(
    state: State<'_, AgentsState>,
    id: String,
    model: Option<String>,
) -> Result<DriverTuning, String> {
    let state = state.inner().clone();
    blocking(move || state.set_model(&id, model)).await
}

#[tauri::command]
pub async fn set_driver_effort(
    state: State<'_, AgentsState>,
    id: String,
    effort: Option<String>,
) -> Result<DriverTuning, String> {
    let state = state.inner().clone();
    blocking(move || state.set_effort(&id, effort)).await
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tutordocs_agent::registry::{Detected, DriverStatus};
    use tutordocs_agent::{AgentDriver, AuthKind, Capabilities, EventStream, JobHandle, JobSpec};

    use super::AgentsState;
    use crate::reader::{install, ReaderState};

    /// The wire shapes both runtimes agree on; vitest replays the same file.
    fn contract() -> Value {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ui/src/fixtures/agents-contract.json"
        ));
        serde_json::from_str(text).expect("contract fixture is valid JSON")
    }

    /// A driver that only answers questions about itself. The picker never
    /// starts a job, and a real handle cannot be built outside the agent crate.
    struct Stub(&'static str);

    #[async_trait::async_trait]
    impl AgentDriver for Stub {
        fn id(&self) -> &'static str {
            self.0
        }
        fn capabilities(&self) -> Capabilities {
            capabilities()
        }
        async fn start(&self, _: JobSpec) -> anyhow::Result<JobHandle> {
            anyhow::bail!("stub driver")
        }
        async fn resume(&self, _: &str, _: JobSpec) -> anyhow::Result<JobHandle> {
            anyhow::bail!("stub driver")
        }
        fn events(&self, _: &JobHandle) -> EventStream {
            unreachable!("stub driver never starts a job")
        }
        async fn cancel(&self, _: &JobHandle) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn capabilities() -> Capabilities {
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

    fn present(id: &'static str, version: &str) -> Detected {
        Detected {
            status: DriverStatus {
                id: id.to_string(),
                available: true,
                binary: Some(format!("/opt/fake/bin/{id}").into()),
                version: Some(version.to_string()),
                capabilities: Some(capabilities()),
                reason: None,
            },
            driver: Some(Arc::new(Stub(id))),
        }
    }

    fn not_found(id: &str) -> Detected {
        Detected {
            status: DriverStatus {
                id: id.to_string(),
                available: false,
                binary: None,
                version: None,
                capabilities: None,
                reason: Some(format!(
                    "{id} CLI not found on PATH or in the known install dirs"
                )),
            },
            driver: None,
        }
    }

    fn unusable(id: &str, why: &str) -> Detected {
        Detected {
            status: DriverStatus {
                id: id.to_string(),
                available: false,
                binary: Some(format!("/opt/fake/bin/{id}").into()),
                version: None,
                capabilities: None,
                reason: Some(format!("{id} CLI found but unusable: {why}")),
            },
            driver: None,
        }
    }

    /// claude present, qoder absent, agy present but broken: the three shapes
    /// the picker has to tell apart.
    fn mixed_machine() -> Vec<Detected> {
        vec![
            present("claude", "2.1.319"),
            not_found("qoder"),
            unusable("agy", "--version exited with status 1"),
        ]
    }

    fn library() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let course =
            json!({"id": "bio101", "title": "Cell Biology", "objectives": [], "chapters": []});
        std::fs::write(dir.path().join("course.json"), course.to_string()).unwrap();
        dir
    }

    fn state_for(root: &Path, machine: fn() -> Vec<Detected>) -> AgentsState {
        AgentsState::new(root, Box::new(machine))
    }

    /// An `AgentsState` whose only driver is the real `ClaudeDriver` over the
    /// fake `claude` script at `binary`. Probes that script once.
    pub(crate) fn claude_agents(root: &Path, binary: &Path) -> AgentsState {
        use tutordocs_agent::ClaudeDriver;
        let probed = Arc::new(ClaudeDriver::new(binary).expect("fake claude probes"));
        let version = probed.version().to_string();
        let caps = probed.capabilities();
        let binary = binary.to_path_buf();
        AgentsState::new(
            root,
            Box::new(move || {
                vec![Detected {
                    status: DriverStatus {
                        id: "claude".to_string(),
                        available: true,
                        binary: Some(binary.clone()),
                        version: Some(version.clone()),
                        capabilities: Some(caps.clone()),
                        reason: None,
                    },
                    driver: Some(probed.clone()),
                }]
            }),
        )
    }

    /// An `AgentsState` that reports every CLI absent.
    pub(crate) fn no_agents(root: &Path) -> AgentsState {
        AgentsState::new(
            root,
            Box::new(|| vec![not_found("claude"), not_found("qoder"), not_found("agy")]),
        )
    }

    /// One command through the Tauri IPC layer, as the webview's `invoke` does.
    pub(crate) fn invoke(
        root: &Path,
        state: AgentsState,
        cmd: &str,
        args: Value,
    ) -> Result<Value, Value> {
        let app = install(mock_builder(), ReaderState::open(root))
            .manage(state)
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

    #[test]
    fn the_picker_lists_all_three_drivers_and_defaults_to_the_first_available() {
        let dir = library();
        let state = state_for(dir.path(), mixed_machine);
        let response = invoke(dir.path(), state, "list_agent_drivers", json!({})).unwrap();
        assert_eq!(response, contract()["list_agent_drivers"]);
    }

    fn with_qoder() -> Vec<Detected> {
        vec![
            present("claude", "2.1.319"),
            present("qoder", "1.1.85"),
            not_found("agy"),
        ]
    }

    fn selected(response: &Value) -> &Value {
        &response["selected"]
    }

    #[test]
    fn a_chosen_driver_is_still_chosen_after_the_app_restarts() {
        let dir = library();
        let state = state_for(dir.path(), with_qoder);
        let chosen = invoke(
            dir.path(),
            state,
            "set_selected_driver",
            json!({"id": "qoder"}),
        )
        .unwrap();
        assert_eq!(selected(&chosen), "qoder");

        let restarted = state_for(dir.path(), with_qoder);
        let listed = invoke(dir.path(), restarted, "list_agent_drivers", json!({})).unwrap();
        assert_eq!(selected(&listed), "qoder");
    }

    #[test]
    fn an_absent_or_unknown_driver_cannot_be_chosen_and_the_choice_stays() {
        let dir = library();
        let state = state_for(dir.path(), mixed_machine);
        for id in ["qoder", "agy", "codex"] {
            let error = invoke(
                dir.path(),
                state.clone(),
                "set_selected_driver",
                json!({"id": id}),
            )
            .unwrap_err();
            assert!(!error.to_string().is_empty(), "{id}");
        }
        let error = invoke(
            dir.path(),
            state.clone(),
            "set_selected_driver",
            json!({"id": "qoder"}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("not available"), "{error}");
        let listed = invoke(dir.path(), state, "list_agent_drivers", json!({})).unwrap();
        assert_eq!(selected(&listed), "claude");
        assert!(!dir.path().join(".tutordocs/agent.json").exists());
    }

    #[test]
    fn a_saved_choice_that_is_no_longer_installed_falls_back_to_the_first_available() {
        let dir = library();
        std::fs::create_dir_all(dir.path().join(".tutordocs")).unwrap();
        std::fs::write(
            dir.path().join(".tutordocs/agent.json"),
            r#"{"driver":"qoder"}"#,
        )
        .unwrap();
        let state = state_for(dir.path(), mixed_machine);
        let listed = invoke(dir.path(), state, "list_agent_drivers", json!({})).unwrap();
        assert_eq!(selected(&listed), "claude");
    }

    #[test]
    fn an_unreadable_choice_file_is_no_choice_rather_than_an_error() {
        let dir = library();
        std::fs::create_dir_all(dir.path().join(".tutordocs")).unwrap();
        std::fs::write(dir.path().join(".tutordocs/agent.json"), "{not json").unwrap();
        let state = state_for(dir.path(), with_qoder);
        let listed = invoke(dir.path(), state, "list_agent_drivers", json!({})).unwrap();
        assert_eq!(selected(&listed), "claude");
    }

    #[test]
    fn no_installed_driver_lists_all_three_absent_with_nothing_selected() {
        let dir = library();
        let state = AgentsState::new(
            dir.path(),
            Box::new(|| vec![not_found("claude"), not_found("qoder"), not_found("agy")]),
        );
        let listed = invoke(dir.path(), state, "list_agent_drivers", json!({})).unwrap();
        assert_eq!(listed["drivers"].as_array().unwrap().len(), 3);
        assert_eq!(selected(&listed), &Value::Null);
    }

    #[test]
    fn refresh_finds_a_cli_installed_since_the_first_probe_and_listing_does_not_reprobe() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let dir = library();
        let installed = Arc::new(AtomicBool::new(false));
        let probes = Arc::new(AtomicUsize::new(0));
        let (flag, count) = (installed.clone(), probes.clone());
        let state = AgentsState::new(
            dir.path(),
            Box::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                if flag.load(Ordering::SeqCst) {
                    vec![
                        present("claude", "2.1.319"),
                        present("qoder", "1.1.85"),
                        not_found("agy"),
                    ]
                } else {
                    vec![
                        present("claude", "2.1.319"),
                        not_found("qoder"),
                        not_found("agy"),
                    ]
                }
            }),
        );
        let first = invoke(dir.path(), state.clone(), "list_agent_drivers", json!({})).unwrap();
        installed.store(true, Ordering::SeqCst);
        let again = invoke(dir.path(), state.clone(), "list_agent_drivers", json!({})).unwrap();
        assert_eq!(first, again, "a listing serves the cache");
        assert_eq!(probes.load(Ordering::SeqCst), 1);

        let refreshed = invoke(dir.path(), state, "refresh_agent_drivers", json!({})).unwrap();
        assert_eq!(refreshed["drivers"][1]["available"], true);
        assert_eq!(probes.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_job_runs_on_the_driver_the_picker_shows() {
        let dir = library();
        std::fs::create_dir_all(dir.path().join(".tutordocs")).unwrap();
        let state = state_for(dir.path(), with_qoder);
        let run = |state: &AgentsState| tauri::async_runtime::block_on(state.driver());
        assert_eq!(run(&state).unwrap().id(), "claude");
        state.select("qoder").unwrap();
        assert_eq!(run(&state).unwrap().id(), "qoder");
    }

    fn only_default(custom: bool) -> Value {
        json!({
            "models": [{"id": null, "label": "CLI default", "efforts": []}],
            "efforts": [],
            "accepts_custom_model": custom
        })
    }

    #[test]
    fn claude_lists_the_aliases_its_help_names() {
        let dir = library();
        let help = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../crates/agent/tests/fixtures/claude/help.txt"
        );
        let binary = executable(
            dir.path(),
            "claude",
            &format!("#!/bin/sh\nif [ \"$1\" = --help ]; then cat '{help}'; exit 0; fi\nexit 0\n"),
        );
        let listed = invoke(
            dir.path(),
            agents_with(
                dir.path(),
                vec![claude_at(&binary), not_found("qoder"), not_found("agy")],
            ),
            "list_driver_models",
            json!({"id": "claude"}),
        )
        .unwrap();
        assert_eq!(listed["accepts_custom_model"], true);
        assert_eq!(
            listed["efforts"],
            json!(["low", "medium", "high", "xhigh", "max"])
        );
        let ids: Vec<Option<&str>> = listed["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str())
            .collect();
        assert_eq!(ids, [None, Some("fable"), Some("opus"), Some("sonnet")]);
        assert!(listed["models"]
            .as_array()
            .unwrap()
            .iter()
            .all(|model| model["efforts"] == listed["efforts"]));

        let missing = invoke(
            dir.path(),
            state_for(dir.path(), mixed_machine),
            "list_driver_models",
            json!({"id": "claude"}),
        )
        .unwrap();
        assert_eq!(missing, only_default(true));
    }

    fn claude_at(binary: &std::path::Path) -> Detected {
        Detected {
            status: DriverStatus {
                id: "claude".to_string(),
                available: true,
                binary: Some(binary.to_path_buf()),
                version: Some("2.1.319".to_string()),
                capabilities: Some(capabilities()),
                reason: None,
            },
            driver: Some(Arc::new(Stub("claude"))),
        }
    }

    #[test]
    fn qoder_lists_only_the_cli_default() {
        let dir = library();
        let state = state_for(dir.path(), with_qoder);
        let listed = invoke(
            dir.path(),
            state,
            "list_driver_models",
            json!({"id": "qoder"}),
        )
        .unwrap();
        assert_eq!(listed, only_default(false));
    }

    #[test]
    fn an_unknown_driver_has_no_model_list() {
        let dir = library();
        let state = state_for(dir.path(), mixed_machine);
        let error = invoke(
            dir.path(),
            state,
            "list_driver_models",
            json!({"id": "codex"}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("not an agent"), "{error}");
    }

    fn executable(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn agents_with(root: &Path, machine: Vec<Detected>) -> AgentsState {
        AgentsState::new(root, Box::new(move || machine.clone()))
    }

    #[test]
    fn agy_lists_base_ids_from_agy_models_and_a_failure_is_the_cli_default() {
        let dir = library();
        let binary = executable(
            dir.path(),
            "agy",
            "#!/bin/sh\nif [ \"$1\" = --help ]; then printf '%s\\n' '  --effort  (low|high)'; exit 0; fi\nif [ \"$1\" = models ]; then printf '%s\\n' 'Fetching available models...' 'gemini-4-argon-high\tGemini 4 Argon (High)' 'gemini-4-argon-low\tGemini 4 Argon (Low)'; exit 0; fi\nexit 1\n",
        );
        let listed = invoke(
            dir.path(),
            agents_with(
                dir.path(),
                vec![
                    present("claude", "2.1.319"),
                    not_found("qoder"),
                    agy_at(&binary),
                ],
            ),
            "list_driver_models",
            json!({"id": "agy"}),
        )
        .unwrap();
        assert_eq!(
            listed,
            json!({
                "models": [
                    {"id": null, "label": "CLI default", "efforts": ["low", "high"]},
                    {"id": "gemini-4-argon", "label": "Gemini 4 Argon", "efforts": ["low", "high"]}
                ],
                "efforts": ["low", "high"],
                "accepts_custom_model": false
            })
        );

        let broken = executable(dir.path(), "agy-broken", "#!/bin/sh\nexit 1\n");
        let failed = invoke(
            dir.path(),
            agents_with(
                dir.path(),
                vec![
                    present("claude", "2.1.319"),
                    not_found("qoder"),
                    agy_at(&broken),
                ],
            ),
            "list_driver_models",
            json!({"id": "agy"}),
        )
        .unwrap();
        assert_eq!(failed, only_default(false));

        let missing = invoke(
            dir.path(),
            agents_with(
                dir.path(),
                vec![
                    present("claude", "2.1.319"),
                    not_found("qoder"),
                    not_found("agy"),
                ],
            ),
            "list_driver_models",
            json!({"id": "agy"}),
        )
        .unwrap();
        assert_eq!(missing, only_default(false));
    }

    #[test]
    fn a_saved_effort_the_new_model_lacks_is_cleared() {
        let dir = library();
        let binary = executable(
            dir.path(),
            "agy-efforts",
            "#!/bin/sh\nif [ \"$1\" = --help ]; then printf '%s\\n' '  --effort  (low|medium|high)'; exit 0; fi\nif [ \"$1\" = models ]; then printf '%s\\n' 'gemini-3.8-flash-high\tGemini 3.8 Flash (High)' 'gemini-3.8-flash-low\tGemini 3.8 Flash (Low)' 'gpt-oss-120b-medium\tGPT-OSS 120B (Medium)'; exit 0; fi\nexit 1\n",
        );
        let state = agents_with(
            dir.path(),
            vec![not_found("claude"), not_found("qoder"), agy_at(&binary)],
        );
        let kept = invoke(
            dir.path(),
            state.clone(),
            "set_driver_effort",
            json!({"id": "agy", "effort": "high"}),
        )
        .unwrap();
        assert_eq!(kept["effort"], "high");
        let flash = invoke(
            dir.path(),
            state.clone(),
            "set_driver_model",
            json!({"id": "agy", "model": "gemini-3.8-flash"}),
        )
        .unwrap();
        assert_eq!(flash["model"], "gemini-3.8-flash");
        assert_eq!(flash["effort"], "high");
        let oss = invoke(
            dir.path(),
            state,
            "set_driver_model",
            json!({"id": "agy", "model": "gpt-oss-120b"}),
        )
        .unwrap();
        assert_eq!(oss["model"], "gpt-oss-120b");
        assert_eq!(oss["effort"], Value::Null);
    }

    fn agy_at(binary: &std::path::Path) -> Detected {
        Detected {
            status: DriverStatus {
                id: "agy".to_string(),
                available: true,
                binary: Some(binary.to_path_buf()),
                version: Some("1.5.3".to_string()),
                capabilities: Some(capabilities()),
                reason: None,
            },
            driver: Some(Arc::new(Stub("agy"))),
        }
    }

    #[test]
    fn a_model_and_effort_survive_a_fresh_agents_state() {
        let dir = library();
        let state = state_for(dir.path(), with_qoder);
        let set = invoke(
            dir.path(),
            state.clone(),
            "set_driver_model",
            json!({"id": "claude", "model": "opus"}),
        )
        .unwrap();
        assert_eq!(set["model"], "opus");
        assert_eq!(set["effort"], Value::Null);
        let set = invoke(
            dir.path(),
            state.clone(),
            "set_driver_effort",
            json!({"id": "claude", "effort": "xhigh"}),
        )
        .unwrap();
        assert_eq!(set["model"], "opus");
        assert_eq!(set["effort"], "xhigh");
        // Clearing the model leaves the effort, and choosing a driver keeps both.
        let cleared = invoke(
            dir.path(),
            state.clone(),
            "set_driver_model",
            json!({"id": "claude", "model": null}),
        )
        .unwrap();
        assert_eq!(cleared["model"], Value::Null);
        assert_eq!(cleared["effort"], "xhigh");
        invoke(
            dir.path(),
            state,
            "set_selected_driver",
            json!({"id": "qoder"}),
        )
        .unwrap();

        let restarted = state_for(dir.path(), with_qoder);
        let tuning = invoke(
            dir.path(),
            restarted.clone(),
            "driver_tuning",
            json!({"id": "claude"}),
        )
        .unwrap();
        assert_eq!(tuning["model"], Value::Null);
        assert_eq!(tuning["effort"], "xhigh");
        let listed = invoke(dir.path(), restarted, "list_agent_drivers", json!({})).unwrap();
        assert_eq!(selected(&listed), "qoder");
    }

    #[test]
    fn a_model_that_starts_with_a_dash_is_rejected_and_not_stored() {
        let dir = library();
        let state = state_for(dir.path(), with_qoder);
        let dashed = invoke(
            dir.path(),
            state.clone(),
            "set_driver_model",
            json!({"id": "claude", "model": "--opus"}),
        )
        .unwrap_err();
        assert!(
            dashed.to_string().contains("must not start with '-'"),
            "{dashed}"
        );
        for model in ["", "opus sonnet"] {
            let error = invoke(
                dir.path(),
                state.clone(),
                "set_driver_model",
                json!({"id": "claude", "model": model}),
            )
            .unwrap_err();
            assert!(!error.to_string().is_empty(), "{model}: {error}");
        }
        let tuning = invoke(dir.path(), state, "driver_tuning", json!({"id": "claude"})).unwrap();
        assert_eq!(tuning["model"], Value::Null);
        assert_eq!(tuning["effort"], Value::Null);
    }

    #[test]
    fn an_unreadable_settings_file_means_no_model_or_effort() {
        let dir = library();
        let state = state_for(dir.path(), with_qoder);
        invoke(
            dir.path(),
            state.clone(),
            "set_selected_driver",
            json!({"id": "qoder"}),
        )
        .unwrap();
        invoke(
            dir.path(),
            state,
            "set_driver_effort",
            json!({"id": "agy", "effort": "max"}),
        )
        .unwrap();
        std::fs::write(dir.path().join(".tutordocs/agent.json"), "{not json").unwrap();

        let restarted = state_for(dir.path(), with_qoder);
        let tuning = invoke(
            dir.path(),
            restarted.clone(),
            "driver_tuning",
            json!({"id": "agy"}),
        )
        .unwrap();
        assert_eq!(tuning["model"], Value::Null);
        assert_eq!(tuning["effort"], Value::Null);
        let listed = invoke(dir.path(), restarted, "list_agent_drivers", json!({})).unwrap();
        assert_eq!(selected(&listed), "claude");
    }
}
