//! Source commands: add a web page to the library's `sources/` as markdown.
//!
//! The agent does the fetching (`tutordocs_engine::ingest_url`); this module
//! finds the library, takes the driver the user chose, and turns the result
//! into the shape the webview reads. A fetch can run for minutes, so the
//! command is async and holds no lock across the job.

use std::time::SystemTime;

use tauri::State;
pub use tutordocs_store::SourceDto;
use tutordocs_store::{
    list_course_templates as store_templates, CourseSetup, CourseSource, ImportResult, Template,
};

use tutordocs_engine::job_scope;

use crate::agents::AgentsState;
use crate::jobs::JobRegistry;
use crate::reader::ReaderState;

/// Fetch `url` through the chosen agent and add it to the library. The error
/// is the reason, already worded for the user.
#[tauri::command]
pub async fn ingest_url(
    reader: State<'_, ReaderState>,
    agents: State<'_, AgentsState>,
    jobs: State<'_, JobRegistry>,
    url: String,
) -> Result<SourceDto, String> {
    let root = reader.with(|library| Ok(library.root().to_path_buf()))?;
    let driver = agents.driver().await?;
    let source = job_scope(
        jobs.log(),
        tutordocs_engine::ingest_url(&root, driver.as_ref(), &url, SystemTime::now()),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(source.into())
}

#[tauri::command]
pub fn list_course_templates() -> Vec<Template> {
    store_templates()
}

#[tauri::command]
pub fn get_course_setup(
    state: State<'_, ReaderState>,
    course_id: String,
) -> Result<Option<CourseSetup>, String> {
    state.with_course(&course_id, |library| {
        library.course_setup().map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn set_course_setup(
    state: State<'_, ReaderState>,
    course_id: String,
    setup: CourseSetup,
) -> Result<CourseSetup, String> {
    state.with_course(&course_id, |library| {
        library.set_course_setup(&setup).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn import_files(
    state: State<'_, ReaderState>,
    course_id: String,
    paths: Vec<String>,
) -> Result<Vec<ImportResult>, String> {
    state.with_course(&course_id, |library| {
        library
            .import_file_strings(&paths)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn list_course_sources(
    state: State<'_, ReaderState>,
    course_id: String,
) -> Result<Vec<CourseSource>, String> {
    state.with_course(&course_id, |library| {
        library.course_sources().map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use serde_json::{json, Value};
    use tutordocs_agent::registry;
    use tutordocs_store::Library;

    use crate::agents::tests::invoke;
    use crate::agents::AgentsState;

    fn contract() -> Value {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ui/src/fixtures/sources-contract.json"
        ));
        serde_json::from_str(text).expect("contract fixture is valid JSON")
    }

    /// A temp library, and an agent registry whose only CLI is a fake `claude`
    /// script that follows `mode`.
    struct Env {
        _tmp: tempfile::TempDir,
        library: PathBuf,
        url_log: PathBuf,
        state: AgentsState,
    }

    fn setup(mode: &str) -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("library");
        Library::create(&library).unwrap();
        let home = tmp.path().join("home");
        let bin = home.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let url_log = tmp.path().join("urls.log");
        let script = format!(
            "#!/bin/sh\nFAKE_MODE='{mode}'\nFAKE_URL_LOG='{}'\n{}",
            url_log.display(),
            include_str!("../tests/fixtures/fake-claude.sh")
                .strip_prefix("#!/bin/sh\n")
                .unwrap()
        );
        let claude = bin.join("claude");
        std::fs::write(&claude, script).unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        let state = AgentsState::new(
            &library,
            Box::new(move || registry::detect(&home, Some(OsStr::new("/usr/bin:/bin")))),
        );
        Env {
            _tmp: tmp,
            library,
            url_log,
            state,
        }
    }

    #[test]
    fn a_pasted_url_becomes_a_source_the_library_lists_with_its_origin() {
        let env = setup("write");
        let url = "https://example.com/a";
        let response = invoke(
            &env.library,
            env.state.clone(),
            "ingest_url",
            json!({"url": url}),
        )
        .unwrap();

        let expected = contract()["ingest_url"].clone();
        let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys(&response), keys(&expected));
        assert_eq!(response["name"], expected["name"]);
        assert_eq!(response["url"], url);
        let fetched = response["fetchedAt"].as_str().unwrap();
        assert!(fetched.ends_with('Z') && fetched.contains('T'), "{fetched}");

        let course = tutordocs_store::Shelf::open(&env.library)
            .unwrap()
            .active_course_dir()
            .unwrap();
        let library = Library::open(&course).unwrap();
        let listed = library.sources().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "example-com-a.md");
        assert_eq!(listed[0].origin.as_ref().unwrap().url, url);
        assert_eq!(
            library.read_source("example-com-a.md").unwrap(),
            "# Example page\n\nFetched by the fake agent.\n"
        );
        assert_eq!(
            response["path"].as_str().unwrap(),
            listed[0].path.to_string_lossy()
        );
    }

    #[test]
    fn a_failed_fetch_reports_why_and_leaves_sources_untouched() {
        let env = setup("fail");
        let error = invoke(
            &env.library,
            env.state.clone(),
            "ingest_url",
            json!({"url": "https://example.com/gone"}),
        )
        .unwrap_err();
        let message = error.as_str().expect("the error is a plain message");
        assert!(message.contains("HTTP 404"), "{message}");
        assert!(!env.library.join("sources").exists());
        let course = tutordocs_store::Shelf::open(&env.library)
            .unwrap()
            .active_course_dir()
            .unwrap();
        assert!(Library::open(&course)
            .unwrap()
            .sources()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn an_address_that_is_not_http_is_refused_before_any_agent_runs() {
        let env = setup("write");
        let error = invoke(
            &env.library,
            env.state.clone(),
            "ingest_url",
            json!({"url": "file:///etc/passwd"}),
        )
        .unwrap_err();
        assert!(error.as_str().unwrap().contains("cannot ingest"), "{error}");
        assert!(!env.url_log.exists(), "the agent must not have run");
    }

    #[test]
    fn with_no_agent_installed_the_message_says_what_to_install() {
        let env = setup("write");
        let none = AgentsState::new(
            &env.library,
            Box::new(|| {
                ["claude", "qoder", "agy"]
                    .map(|id| tutordocs_agent::registry::Detected {
                        status: tutordocs_agent::registry::DriverStatus {
                            id: id.to_string(),
                            available: false,
                            binary: None,
                            version: None,
                            capabilities: None,
                            reason: Some("not found".to_string()),
                        },
                        driver: None,
                    })
                    .into()
            }),
        );
        let error = invoke(
            &env.library,
            none,
            "ingest_url",
            json!({"url": "https://example.com/a"}),
        )
        .unwrap_err();
        assert!(error.as_str().unwrap().contains("No agent CLI"), "{error}");
        assert!(!env.url_log.exists());
    }

    #[test]
    fn list_course_templates_and_course_setup_work_over_ipc() {
        let env = setup("write");
        let course = invoke(
            &env.library,
            env.state.clone(),
            "create_course",
            json!({"title": "Distributed Systems"}),
        )
        .unwrap();
        let course_id = course["id"].as_str().unwrap();

        let templates = invoke(
            &env.library,
            env.state.clone(),
            "list_course_templates",
            json!({}),
        )
        .unwrap();
        let ids: Vec<&str> = templates
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["interview", "exam", "overview", "custom"]);

        let initial = invoke(
            &env.library,
            env.state.clone(),
            "get_course_setup",
            json!({"courseId": course_id}),
        )
        .unwrap();
        assert_eq!(initial, json!(null));

        let payload = json!({
            "template": "interview",
            "level": "some",
            "hoursPerWeek": 5,
            "language": "th",
            "guide": "Ask about Paxos and Raft."
        });
        let saved = invoke(
            &env.library,
            env.state.clone(),
            "set_course_setup",
            json!({"courseId": course_id, "setup": payload}),
        )
        .unwrap();
        assert_eq!(saved, payload);

        let read_back = invoke(
            &env.library,
            env.state.clone(),
            "get_course_setup",
            json!({"courseId": course_id}),
        )
        .unwrap();
        assert_eq!(read_back, payload);

        let bad_template = invoke(
            &env.library,
            env.state.clone(),
            "set_course_setup",
            json!({
                "courseId": course_id,
                "setup": {
                    "template": "nonexistent",
                    "level": "new",
                    "hoursPerWeek": null,
                    "language": "en",
                    "guide": ""
                }
            }),
        );
        assert!(bad_template.is_err());

        let bad_lang = invoke(
            &env.library,
            env.state.clone(),
            "set_course_setup",
            json!({
                "courseId": course_id,
                "setup": {
                    "template": "exam",
                    "level": "new",
                    "hoursPerWeek": null,
                    "language": "de",
                    "guide": ""
                }
            }),
        );
        assert!(bad_lang.is_err());
    }

    #[test]
    fn import_files_and_list_course_sources_work_over_ipc_and_pdf_extracts_via_existing_path() {
        use tutordocs_ingest::fixture::Page;

        let env = setup("write");
        let course = invoke(
            &env.library,
            env.state.clone(),
            "create_course",
            json!({"title": "Compilers"}),
        )
        .unwrap();
        let course_id = course["id"].as_str().unwrap();
        invoke(
            &env.library,
            env.state.clone(),
            "mark_course_opened",
            json!({"courseId": course_id}),
        )
        .unwrap();

        let incoming = env._tmp.path().join("incoming");
        std::fs::create_dir_all(&incoming).unwrap();
        let pdf_bytes = tutordocs_ingest::fixture::pdf(&[Page::text(
            "BT /F1 24 Tf 72 720 Td (Hello world) Tj ET",
        )]);
        let pdf_path = incoming.join("lecture.pdf");
        let md_path = incoming.join("notes.md");
        let txt_path = incoming.join("syllabus.txt");
        let bad_path = incoming.join("archive.zip");
        std::fs::write(&pdf_path, &pdf_bytes).unwrap();
        std::fs::write(&md_path, "# Parsing\n").unwrap();
        std::fs::write(&txt_path, "Week 1: Lexing\n").unwrap();
        std::fs::write(&bad_path, b"PK\x03\x04").unwrap();

        let results = invoke(
            &env.library,
            env.state.clone(),
            "import_files",
            json!({
                "courseId": course_id,
                "paths": [
                    pdf_path.to_str().unwrap(),
                    md_path.to_str().unwrap(),
                    txt_path.to_str().unwrap(),
                    bad_path.to_str().unwrap(),
                    md_path.to_str().unwrap()
                ]
            }),
        )
        .unwrap();
        let arr = results.as_array().unwrap();
        assert_eq!(arr.len(), 5);
        assert_eq!(arr[0]["status"], "imported");
        assert_eq!(arr[0]["reason"], json!(null));
        assert_eq!(arr[0]["source"]["name"], "lecture.pdf");
        assert_eq!(arr[0]["source"]["kind"], "pdf");
        assert_eq!(arr[0]["source"]["bytes"], pdf_bytes.len());

        assert_eq!(arr[1]["status"], "imported");
        assert_eq!(arr[1]["source"]["kind"], "markdown");

        assert_eq!(arr[2]["status"], "imported");
        assert_eq!(arr[2]["source"]["kind"], "text");

        assert_eq!(arr[3]["status"], "skipped");
        assert!(arr[3]["reason"].is_string());
        assert_eq!(arr[3]["source"], json!(null));

        assert_eq!(arr[4]["status"], "skipped");
        assert_eq!(arr[4]["reason"], "duplicate");

        let listed = invoke(
            &env.library,
            env.state.clone(),
            "list_course_sources",
            json!({"courseId": course_id}),
        )
        .unwrap();
        let names: Vec<&str> = listed
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["lecture.pdf", "notes.md", "syllabus.txt"]);

        // Existing PDF source extraction path reads the imported PDF without a new extractor.
        let extracted = invoke(
            &env.library,
            env.state.clone(),
            "read_pdf_source",
            json!({"courseId": course_id, "name": "lecture.pdf"}),
        )
        .unwrap();
        assert_eq!(extracted["pages"][0]["text"], "Hello world");
    }

    #[test]
    fn capability_grants_only_dialog_allow_open_beside_core_default() {
        let raw = include_str!("../capabilities/reader.json");
        let cap: Value = serde_json::from_str(raw).expect("valid capability json");
        assert_eq!(
            cap["permissions"],
            json!(["core:default", "dialog:allow-open"])
        );
    }

    #[test]
    fn updater_capability_grants_expected_permissions() {
        let raw = include_str!("../capabilities/updater.json");
        let cap: Value = serde_json::from_str(raw).expect("valid updater capability json");
        assert_eq!(
            cap["permissions"],
            json!([
                "dialog:allow-ask",
                "dialog:allow-message",
                "updater:default",
                "process:default"
            ])
        );
        assert_eq!(cap["windows"], json!(["reader"]));
    }
}
