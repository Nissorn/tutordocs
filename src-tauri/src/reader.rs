//! Reader commands: list courses, list chapters, read a chapter's markdown.
//!
//! The sidebar tree comes from `course.json` through the store crate. Markdown
//! crosses the boundary verbatim: the TypeScript UI renders it, Rust never
//! parses it (PROPOSAL "Ingestion and the Rust crate choices"). A chapter with
//! no markdown yet is a normal state, `placeholder`, not an error.

use std::path::Path;
use std::sync::Mutex;

use tauri::State;
pub use tutordocs_store::{
    default_library_root, is_plain_component, resolve_course_dir as course_dir, ChapterDoc,
    ChapterSummary, CourseSummary as LibraryCourse, Library, OutlineCourseSummary as CourseSummary,
    Shelf,
};

/// The open shelf, or the reason it could not be opened. Kept as a value so a
/// bad library path reaches the UI as a readable message instead of a crash.
pub struct ReaderState {
    shelf: Mutex<Result<Shelf, String>>,
}

impl ReaderState {
    /// Open the shelf at `root`, creating it on first launch when the folder
    /// is missing or empty, and remembering the failure message otherwise. A
    /// folder that already holds other files is never turned into a library.
    pub fn open(root: &Path) -> Self {
        Self {
            shelf: Mutex::new(Shelf::open_or_create(root).map_err(|e| e.to_string())),
        }
    }

    pub(crate) fn shelf<T>(
        &self,
        f: impl FnOnce(&Shelf) -> Result<T, String>,
    ) -> Result<T, String> {
        let guard = self
            .shelf
            .lock()
            .map_err(|_| "the library lock was poisoned by an earlier failure".to_string())?;
        match &*guard {
            Ok(shelf) => f(shelf),
            Err(message) => Err(message.clone()),
        }
    }

    /// Run `f` against the course commands without a course id should use.
    pub(crate) fn with<T>(
        &self,
        f: impl FnOnce(&Library) -> Result<T, String>,
    ) -> Result<T, String> {
        self.shelf(|shelf| {
            let dir = shelf.active_course_dir().map_err(|e| e.to_string())?;
            let library = Library::open(dir).map_err(|e| e.to_string())?;
            f(&library)
        })
    }

    /// Run `f` against the course `course_id` names.
    pub(crate) fn with_course<T>(
        &self,
        course_id: &str,
        f: impl FnOnce(&Library) -> Result<T, String>,
    ) -> Result<T, String> {
        self.shelf(|shelf| {
            let library = shelf.open_course(course_id).map_err(|e| e.to_string())?;
            f(&library)
        })
    }
}

/// Register the reader's state and commands. The app and the tests both build
/// through this, so a command added here is the command the UI can invoke.
pub fn install<R: tauri::Runtime>(
    builder: tauri::Builder<R>,
    state: ReaderState,
) -> tauri::Builder<R> {
    let builder = builder
        .manage(state)
        .manage(crate::jobs::JobRegistry::new());
    #[cfg(feature = "e2e-control")]
    let builder = builder.manage(crate::ctl::CtlState::default());
    builder.invoke_handler(tauri::generate_handler![
        crate::agents::list_agent_drivers,
        crate::agents::refresh_agent_drivers,
        crate::agents::set_selected_driver,
        crate::agents::list_driver_models,
        crate::agents::driver_tuning,
        crate::agents::set_driver_model,
        crate::agents::set_driver_effort,
        crate::sources::ingest_url,
        crate::sources::list_course_templates,
        crate::sources::get_course_setup,
        crate::sources::set_course_setup,
        crate::sources::import_files,
        crate::sources::list_course_sources,
        list_courses,
        list_library_courses,
        create_course,
        set_course_tags,
        mark_course_opened,
        list_chapters,
        read_chapter,
        crate::annotations::list_annotations,
        crate::annotations::save_annotation,
        crate::annotations::delete_annotation,
        crate::generate::generate_outline,
        crate::generate::get_outline,
        crate::generate::list_outline_variants,
        crate::generate::get_outline_variant,
        crate::generate::branch_outline,
        crate::generate::set_current_variant,
        crate::generate::save_outline,
        crate::generate::revise_outline,
        crate::generate::approve_outline,
        crate::generate::get_chapter_queue,
        crate::generate::cancel_chapter_queue,
        crate::generate::open_chapter,
        crate::generate::regenerate_chapter,
        crate::generate::revise_section,
        crate::generate::ask_about,
        crate::generate::list_answers,
        crate::generate::chapter_context,
        crate::artifacts::describe_visual,
        crate::artifacts::record_visual_event,
        crate::pdf_sources::list_pdf_sources,
        crate::pdf_sources::read_pdf_source,
        crate::pdf_sources::read_text_source,
        crate::pdf_sources::list_source_annotations,
        crate::pdf_sources::save_source_annotation,
        crate::quiz::generate_quiz,
        crate::quiz::grade_quiz,
        crate::quiz::course_progress,
        crate::jobs::list_agent_jobs,
        crate::jobs::agent_job,
        crate::jobs::cancel_agent_job,
        crate::trash::trash_course,
        crate::trash::list_trash,
        crate::trash::restore_course,
        crate::trash::empty_trash,
        crate::trash::rename_course,
        #[cfg(feature = "e2e-control")]
        crate::ctl::__ctl_result
    ])
}

#[tauri::command]
pub fn list_courses(state: State<'_, ReaderState>) -> Result<Vec<CourseSummary>, String> {
    state.shelf(|shelf| shelf.list_outline_courses().map_err(|e| e.to_string()))
}

#[tauri::command]
pub fn list_library_courses(
    state: State<'_, ReaderState>,
    jobs: State<'_, crate::jobs::JobRegistry>,
) -> Result<Vec<LibraryCourse>, String> {
    state.shelf(|shelf| {
        let mut courses = shelf.list().map_err(|e| e.to_string())?;
        let running_jobs = jobs.list();
        for course in &mut courses {
            let is_planning = running_jobs.iter().any(|job| {
                job.status == tutordocs_engine::JobStatus::Running
                    && job.kind == tutordocs_engine::JobKind::Outline
                    && (job.course_id == course.id
                        || (!course.stored_id.is_empty() && job.course_id == course.stored_id))
            });
            if is_planning {
                course.planning = true;
            }
        }
        Ok(courses)
    })
}

#[tauri::command]
pub fn create_course(
    state: State<'_, ReaderState>,
    title: String,
    tags: Option<Vec<String>>,
) -> Result<LibraryCourse, String> {
    state.shelf(|shelf| {
        shelf
            .create_course(&title, tags.as_deref().unwrap_or(&[]))
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn set_course_tags(
    state: State<'_, ReaderState>,
    course_id: String,
    tags: Vec<String>,
) -> Result<LibraryCourse, String> {
    state.shelf(|shelf| shelf.set_tags(&course_id, &tags).map_err(|e| e.to_string()))
}

#[tauri::command]
pub fn mark_course_opened(
    state: State<'_, ReaderState>,
    course_id: String,
) -> Result<LibraryCourse, String> {
    state.shelf(|shelf| shelf.mark_opened(&course_id).map_err(|e| e.to_string()))
}

#[tauri::command]
pub fn list_chapters(
    state: State<'_, ReaderState>,
    course_id: String,
) -> Result<Vec<ChapterSummary>, String> {
    state.with_course(&course_id, |library| {
        library.list_chapter_summaries().map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn read_chapter(
    state: State<'_, ReaderState>,
    course_id: String,
    chapter_id: String,
) -> Result<ChapterDoc, String> {
    state.with_course(&course_id, |library| {
        library.read_chapter_doc(&course_id, &chapter_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;

    /// The wire shapes both runtimes agree on; vitest replays the same file.
    fn contract() -> Value {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ui/src/fixtures/reader-contract.json"
        ));
        serde_json::from_str(text).expect("contract fixture is valid JSON")
    }

    /// A library whose chapters match the contract fixture: `cells` has an
    /// `index.md`, `membranes` is an empty directory.
    fn temp_library() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let contract = contract();
        let course = json!({
            "id": "bio101",
            "title": "Cell Biology",
            "objectives": contract["list_courses"][0]["objectives"],
            "chapters": [
                {"id": "cells", "title": "Cells"},
                {"id": "membranes", "title": "Membranes"}
            ]
        });
        std::fs::write(dir.path().join("course.json"), course.to_string()).unwrap();
        let cells = dir.path().join("chapters/cells");
        std::fs::create_dir_all(&cells).unwrap();
        let markdown = contract["read_chapter_ready"]["markdown"].as_str().unwrap();
        std::fs::write(cells.join("index.md"), markdown).unwrap();
        std::fs::create_dir_all(dir.path().join("chapters/membranes")).unwrap();
        dir
    }

    /// Run one command through the Tauri IPC layer, as the webview's `invoke` does.
    fn invoke(root: &Path, cmd: &str, args: Value) -> Result<Value, Value> {
        let app = install(mock_builder(), ReaderState::open(root))
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
    fn list_courses_returns_the_outline_course() {
        let dir = temp_library();
        let response = invoke(dir.path(), "list_courses", json!({})).unwrap();
        assert_eq!(response, contract()["list_courses"]);
    }

    #[test]
    fn list_library_courses_matches_the_contract() {
        let dir = temp_library();
        let mut response = invoke(dir.path(), "list_library_courses", json!({})).unwrap();
        // The folder's creation time is the clock of the test run, which a fixture cannot hold.
        response[0]["createdAt"] = contract()["list_library_courses"][0]["createdAt"].clone();
        assert_eq!(response, contract()["list_library_courses"]);
    }

    #[test]
    fn list_chapters_flags_the_placeholder_in_outline_order() {
        let dir = temp_library();
        let args = json!({"courseId": "bio101"});
        let response = invoke(dir.path(), "list_chapters", args).unwrap();
        assert_eq!(response, contract()["list_chapters"]);
    }

    #[test]
    fn read_chapter_returns_the_markdown_verbatim() {
        let dir = temp_library();
        let args = json!({"courseId": "bio101", "chapterId": "cells"});
        let response = invoke(dir.path(), "read_chapter", args).unwrap();
        assert_eq!(response, contract()["read_chapter_ready"]);
    }

    #[test]
    fn read_chapter_of_a_placeholder_succeeds_with_the_placeholder_state() {
        let dir = temp_library();
        let args = json!({"courseId": "bio101", "chapterId": "membranes"});
        let response = invoke(dir.path(), "read_chapter", args)
            .expect("a placeholder is a normal state, not an error");
        assert_eq!(response, contract()["read_chapter_placeholder"]);
    }

    #[test]
    fn read_chapter_refuses_unknown_courses_unknown_chapters_and_path_escapes() {
        let dir = temp_library();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("index.md"), "# secret\n").unwrap();
        let course = json!({
            "id": "bio101", "title": "Cell Biology", "objectives": [],
            "chapters": [{"id": "cells", "title": "Cells"}, {"id": "../outside", "title": "Escape"}]
        });
        std::fs::write(dir.path().join("course.json"), course.to_string()).unwrap();

        let unknown_course = json!({"courseId": "nope", "chapterId": "cells"});
        assert!(invoke(dir.path(), "read_chapter", unknown_course).is_err());
        let unknown_chapter = json!({"courseId": "bio101", "chapterId": "nope"});
        assert!(invoke(dir.path(), "read_chapter", unknown_chapter).is_err());
        let escape = json!({"courseId": "bio101", "chapterId": "../outside"});
        let error = invoke(dir.path(), "read_chapter", escape).unwrap_err();
        assert!(error.to_string().contains("invalid chapter id"), "{error}");
    }

    #[test]
    fn a_folder_with_other_files_is_not_turned_into_a_library() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "mine").unwrap();
        let error = invoke(dir.path(), "list_courses", json!({})).unwrap_err();
        assert!(
            error.to_string().contains("not a TutorDocs library"),
            "{error}"
        );
        assert!(!dir.path().join("course.json").exists());
    }

    #[test]
    fn first_launch_creates_the_library_in_a_missing_or_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("TutorDocs");
        for root in [dir.path(), missing.as_path()] {
            let listed = invoke(root, "list_courses", json!({})).unwrap();
            assert_eq!(listed, json!([]));
            assert!(root.join("courses").is_dir());
            assert!(root.join(".tutordocs").is_dir());
            assert!(!root.join("course.json").exists());
        }
    }

    #[test]
    fn two_courses_keep_separate_sources_and_annotations() {
        let dir = tempfile::tempdir().unwrap();
        let biology = invoke(
            dir.path(),
            "create_course",
            json!({"title": "Biology", "tags": ["bio"]}),
        )
        .unwrap();
        let history = invoke(dir.path(), "create_course", json!({"title": "History"})).unwrap();
        assert_eq!(biology["id"], "biology");
        assert_eq!(history["id"], "history");
        assert_eq!(biology["openedAt"], json!(null));
        assert_eq!(biology["error"], json!(null));
        assert_eq!(biology["tags"], json!(["bio"]));
        for key in [
            "id",
            "title",
            "tags",
            "objectives",
            "storedId",
            "chapterCount",
            "chapterTitles",
            "writtenChapterCount",
            "createdAt",
            "openedAt",
            "error",
        ] {
            assert!(biology.get(key).is_some(), "missing {key}");
        }

        invoke(
            dir.path(),
            "mark_course_opened",
            json!({"courseId": "biology"}),
        )
        .unwrap();
        invoke(
            dir.path(),
            "save_annotation",
            json!({
                "chapterId": "cells",
                "mark": {"annotationId": "mark-1", "quote": "cell", "note": "n"}
            }),
        )
        .unwrap();
        let sources = dir.path().join("courses/biology/sources");
        std::fs::write(sources.join("notes.pdf"), b"%PDF").unwrap();
        assert_eq!(
            invoke(
                dir.path(),
                "list_pdf_sources",
                json!({"courseId": "biology"})
            )
            .unwrap(),
            json!(["notes.pdf"])
        );
        assert_eq!(
            invoke(
                dir.path(),
                "list_annotations",
                json!({"chapterId": "cells"})
            )
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
            1
        );

        invoke(
            dir.path(),
            "mark_course_opened",
            json!({"courseId": "history"}),
        )
        .unwrap();
        assert_eq!(
            invoke(
                dir.path(),
                "list_annotations",
                json!({"chapterId": "cells"})
            )
            .unwrap(),
            json!([])
        );
        assert_eq!(
            invoke(
                dir.path(),
                "list_pdf_sources",
                json!({"courseId": "biology"})
            )
            .unwrap(),
            json!(["notes.pdf"])
        );
        assert_eq!(
            invoke(
                dir.path(),
                "list_pdf_sources",
                json!({"courseId": "history"})
            )
            .unwrap(),
            json!([])
        );

        invoke(
            dir.path(),
            "mark_course_opened",
            json!({"courseId": "biology"}),
        )
        .unwrap();
        assert_eq!(
            invoke(
                dir.path(),
                "list_annotations",
                json!({"chapterId": "cells"})
            )
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            invoke(
                dir.path(),
                "list_pdf_sources",
                json!({"courseId": "biology"})
            )
            .unwrap(),
            json!(["notes.pdf"])
        );

        let listed = invoke(dir.path(), "list_library_courses", json!({})).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 2);
    }
}
