//! Course trash and rename commands.
//!
//! Moving a course to trash preserves every byte of its sources, chapters and
//! SQLite database. `trash_course` refuses while an agent job is running or
//! while chapters are queued.

use tauri::State;
use tutordocs_store::{CourseSummary as LibraryCourse, TrashEntry};

use crate::generate::GenerationState;
use crate::jobs::JobRegistry;
use crate::reader::ReaderState;

#[tauri::command]
pub fn trash_course(
    state: State<'_, ReaderState>,
    jobs: State<'_, JobRegistry>,
    generation: State<'_, GenerationState>,
    course_id: String,
) -> Result<TrashEntry, String> {
    let (folder_id, stored_id) = state.shelf(|shelf| {
        let dir = shelf.course_dir(&course_id).map_err(|e| e.to_string())?;
        let folder_id = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&course_id)
            .to_string();
        let stored_id =
            if let Ok(course) = shelf.open_course(&folder_id).and_then(|lib| lib.course()) {
                course.id
            } else {
                String::new()
            };
        Ok((folder_id, stored_id))
    })?;

    // Refuse while any job for that course is running.
    let running_jobs = jobs.list();
    for job in &running_jobs {
        if job.status == tutordocs_engine::JobStatus::Running
            && (job.course_id == folder_id || (!stored_id.is_empty() && job.course_id == stored_id))
        {
            return Err(format!(
                "cannot trash course {course_id}: an agent job is currently running"
            ));
        }
    }

    // Refuse while get_chapter_queue reports running or pending entries.
    if let Ok(queue) = generation.get_chapter_queue(&folder_id) {
        if queue.running.is_some() || !queue.pending.is_empty() {
            return Err(format!(
                "cannot trash course {course_id}: chapter queue is active"
            ));
        }
    }

    state.shelf(|shelf| shelf.trash_course(&course_id).map_err(|e| e.to_string()))
}

#[tauri::command]
pub fn list_trash(state: State<'_, ReaderState>) -> Result<Vec<TrashEntry>, String> {
    state.shelf(|shelf| shelf.list_trash().map_err(|e| e.to_string()))
}

#[tauri::command]
pub fn restore_course(
    state: State<'_, ReaderState>,
    trash_id: String,
) -> Result<LibraryCourse, String> {
    state.shelf(|shelf| shelf.restore_course(&trash_id).map_err(|e| e.to_string()))
}

#[tauri::command]
pub fn empty_trash(state: State<'_, ReaderState>, trash_id: String) -> Result<(), String> {
    state.shelf(|shelf| shelf.empty_trash(&trash_id).map_err(|e| e.to_string()))
}

#[tauri::command]
pub fn rename_course(
    state: State<'_, ReaderState>,
    course_id: String,
    title: String,
) -> Result<LibraryCourse, String> {
    state.shelf(|shelf| {
        shelf
            .rename_course(&course_id, &title)
            .map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tutordocs_engine::{JobKind, JobLog, JobStart};

    fn setup_test_app(
        root: &std::path::Path,
        jobs: JobRegistry,
    ) -> tauri::WebviewWindow<tauri::test::MockRuntime> {
        let agents = crate::agents::AgentsState::installed(root);
        let generation = GenerationState::new(root.to_path_buf(), agents.clone());
        let reader_state = ReaderState::open(root);

        let app = mock_builder()
            .manage(reader_state)
            .manage(jobs)
            .manage(generation)
            .manage(agents)
            .invoke_handler(tauri::generate_handler![
                trash_course,
                list_trash,
                restore_course,
                empty_trash,
                rename_course,
                crate::reader::list_library_courses,
            ])
            .build(mock_context(noop_assets()))
            .expect("mock app builds");

        tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds")
    }

    fn call_ipc(
        webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        cmd: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, serde_json::Value> {
        get_ipc_response(
            webview,
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
        .map(|body| {
            body.deserialize::<serde_json::Value>()
                .expect("JSON response")
        })
    }

    #[test]
    fn trash_course_refuses_while_job_is_running() {
        let tmp = tempfile::tempdir().unwrap();
        let shelf = tutordocs_store::Shelf::open_or_create(tmp.path()).unwrap();
        let course = shelf.create_course("Biology", &[]).unwrap();

        let jobs = JobRegistry::new();
        // Start a running job for this course
        jobs.begin(JobStart {
            kind: JobKind::Outline,
            driver: "fake".into(),
            model: None,
            effort: None,
            command: "fake run".into(),
            course_id: course.id.clone(),
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            wake: Arc::new(|| {}),
        });

        let webview = setup_test_app(tmp.path(), jobs);
        let err = call_ipc(&webview, "trash_course", json!({"courseId": course.id})).unwrap_err();

        let msg = err.as_str().unwrap();
        assert!(
            msg.contains("an agent job is currently running"),
            "expected running job refusal, got: {msg}"
        );
    }

    #[test]
    fn rename_course_through_ipc_updates_title() {
        let tmp = tempfile::tempdir().unwrap();
        let shelf = tutordocs_store::Shelf::open_or_create(tmp.path()).unwrap();
        let course = shelf.create_course("Biology", &[]).unwrap();

        let webview = setup_test_app(tmp.path(), JobRegistry::new());
        let updated = call_ipc(
            &webview,
            "rename_course",
            json!({"courseId": course.id, "title": "Advanced Biology"}),
        )
        .unwrap();

        assert_eq!(updated["id"], "biology");
        assert_eq!(updated["title"], "Advanced Biology");
    }

    #[test]
    fn trash_and_restore_through_ipc_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let shelf = tutordocs_store::Shelf::open_or_create(tmp.path()).unwrap();
        let course = shelf.create_course("Biology", &[]).unwrap();

        let webview = setup_test_app(tmp.path(), JobRegistry::new());
        let trashed = call_ipc(&webview, "trash_course", json!({"courseId": course.id})).unwrap();

        assert_eq!(trashed["courseId"], "biology");
        let trash_id = trashed["trashId"].as_str().unwrap();

        let list = call_ipc(&webview, "list_trash", json!({})).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);

        let restored = call_ipc(&webview, "restore_course", json!({"trashId": trash_id})).unwrap();

        assert_eq!(restored["id"], "biology");
        let list_after = call_ipc(&webview, "list_trash", json!({})).unwrap();
        assert_eq!(list_after.as_array().unwrap().len(), 0);
    }
}
