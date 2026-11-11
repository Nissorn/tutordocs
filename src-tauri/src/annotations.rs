//! Annotation commands: list, save and delete the marks on one chapter. PDF
//! sources (`pdf_sources.rs`) save the same `Mark` under their own material.
//!
//! Rust stores marks and hands them back; it never interprets their positions.
//! Positions index the rendered chapter text in the webview, and the re-anchor
//! runs there (`ui/src/anchor.ts`), so one unit system covers capture, match
//! and paint. A regenerated chapter never deletes a mark from here: the UI
//! flips a mark it cannot place to `unresolved` or `ambiguous` and saves it.

use serde::{Deserialize, Serialize};
use tauri::State;
use tutordocs_store::{Annotation, AnnotationKind, TextSelector};

use crate::reader::{is_plain_component, ReaderState};

/// A mark as the UI sends it. The UI owns the id, so a retried save converges
/// on one row instead of duplicating the mark.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkInput {
    pub annotation_id: String,
    #[serde(default)]
    pub kind: AnnotationKind,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub quote: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub selectors: Vec<TextSelector>,
    #[serde(default)]
    pub resolution: String,
    /// The unit inside the material: the page of a PDF, 1 for a chapter.
    #[serde(default = "first_unit")]
    pub locator: i64,
    #[serde(default)]
    pub rects: Vec<Rect>,
}

fn first_unit() -> i64 {
    1
}

/// A rect normalized to `[0, 1]` fractions of a PDF page, origin bottom left,
/// in the geometry `crates/ingest` extracts. Chapters have none.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A stored mark as the UI reads it, after the store's normalization.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    pub annotation_id: String,
    pub kind: AnnotationKind,
    pub color: String,
    pub quote: String,
    pub note: String,
    pub selectors: Vec<TextSelector>,
    pub resolution: String,
    pub locator: i64,
    pub rects: Vec<Rect>,
    pub created_at: f64,
    pub updated_at: f64,
}

/// The longest annotation id accepted; the UI sends a UUID.
const MAX_ID_CHARS: usize = 128;

impl From<Annotation> for Mark {
    fn from(a: Annotation) -> Self {
        Self {
            annotation_id: a.annotation_id,
            kind: a.kind,
            color: a.color,
            quote: a.quote,
            note: a.note,
            selectors: a.selectors,
            resolution: a.resolution,
            locator: a.locator,
            rects: a
                .rects
                .into_iter()
                .filter_map(|r| serde_json::from_value(r).ok())
                .collect(),
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

/// The store key for a chapter's marks. Sources (ticket 09) get their own
/// prefix, so a chapter and a source sharing an id never share marks.
fn material_id(chapter_id: &str) -> Result<String, String> {
    if is_plain_component(chapter_id) {
        Ok(format!("chapters/{chapter_id}"))
    } else {
        Err(format!("invalid chapter id: {chapter_id:?}"))
    }
}

#[tauri::command]
pub fn list_annotations(
    state: State<'_, ReaderState>,
    chapter_id: String,
) -> Result<Vec<Mark>, String> {
    list_marks(&state, &material_id(&chapter_id)?)
}

#[tauri::command]
pub fn save_annotation(
    state: State<'_, ReaderState>,
    chapter_id: String,
    mark: MarkInput,
) -> Result<Mark, String> {
    save_mark(&state, material_id(&chapter_id)?, mark)
}

pub(crate) fn list_marks(state: &ReaderState, material: &str) -> Result<Vec<Mark>, String> {
    state.with(|library| {
        let marks = library
            .annotations_for(material)
            .map_err(|e| e.to_string())?;
        Ok(marks.into_iter().map(Mark::from).collect())
    })
}

pub(crate) fn save_mark(
    state: &ReaderState,
    material: String,
    mark: MarkInput,
) -> Result<Mark, String> {
    let id = mark.annotation_id;
    if id.is_empty() || id.chars().count() > MAX_ID_CHARS {
        return Err(format!(
            "an annotation id must be 1 to {MAX_ID_CHARS} characters"
        ));
    }
    let annotation = Annotation {
        kind: mark.kind,
        color: mark.color,
        quote: mark.quote,
        note: mark.note,
        selectors: mark.selectors,
        resolution: mark.resolution,
        locator: mark.locator,
        rects: mark
            .rects
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
        ..Annotation::new(id.clone(), material)
    };
    state.with(|library| {
        library
            .put_annotation(&annotation)
            .map_err(|e| e.to_string())?;
        library
            .annotation(&id)
            .map_err(|e| e.to_string())?
            .map(Mark::from)
            .ok_or_else(|| format!("annotation {id:?} vanished after it was saved"))
    })
}

#[tauri::command]
pub fn delete_annotation(
    state: State<'_, ReaderState>,
    annotation_id: String,
) -> Result<bool, String> {
    state.with(|library| {
        library
            .delete_annotation(&annotation_id)
            .map_err(|e| e.to_string())
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;

    use crate::reader::{install, ReaderState};

    /// The wire shapes both runtimes agree on; vitest replays the same file.
    fn contract() -> Value {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ui/src/fixtures/annotations-contract.json"
        ));
        serde_json::from_str(text).expect("contract fixture is valid JSON")
    }

    fn temp_library() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let course = json!({
            "id": "bio101", "title": "Cell Biology", "objectives": [],
            "chapters": [{"id": "cells", "title": "Cells"}]
        });
        std::fs::write(dir.path().join("course.json"), course.to_string()).unwrap();
        let cells = dir.path().join("chapters/cells");
        std::fs::create_dir_all(&cells).unwrap();
        std::fs::write(cells.join("index.md"), "# Cells\n").unwrap();
        dir
    }

    /// One command through the Tauri IPC layer. Every call builds a fresh app
    /// and reopens the library, so a value read back crossed a closed database.
    pub(crate) fn invoke(root: &Path, cmd: &str, args: Value) -> Result<Value, Value> {
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

    /// Timestamps are the clock's; the fixture pins every other field.
    fn pin_timestamps(mut marks: Value) -> Value {
        for mark in marks.as_array_mut().expect("a list of marks") {
            for key in ["createdAt", "updatedAt"] {
                assert!(mark[key].as_f64().unwrap() > 1.7e9, "{key} is unix seconds");
                mark[key] = json!(1_700_000_000);
            }
        }
        marks
    }

    #[test]
    fn a_saved_mark_and_its_note_read_back_after_the_database_is_reopened() {
        let dir = temp_library();
        let saved = invoke(
            dir.path(),
            "save_annotation",
            contract()["save_annotation"].clone(),
        )
        .expect("save succeeds");
        assert_eq!(saved["annotationId"], "mark-1");

        let listed = invoke(
            dir.path(),
            "list_annotations",
            json!({"chapterId": "cells"}),
        )
        .unwrap();
        assert_eq!(pin_timestamps(listed), contract()["list_annotations"]);
    }

    #[test]
    fn saving_the_same_id_again_updates_the_note_and_keeps_one_mark() {
        let dir = temp_library();
        let mut args = contract()["save_annotation"].clone();
        invoke(dir.path(), "save_annotation", args.clone()).unwrap();
        let first = invoke(
            dir.path(),
            "list_annotations",
            json!({"chapterId": "cells"}),
        )
        .unwrap();

        args["mark"]["note"] = json!("Smallest that is still alive.");
        args["mark"]["resolution"] = json!("unresolved");
        invoke(dir.path(), "save_annotation", args).unwrap();

        let listed = invoke(
            dir.path(),
            "list_annotations",
            json!({"chapterId": "cells"}),
        )
        .unwrap();
        let marks = listed.as_array().unwrap();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0]["note"], "Smallest that is still alive.");
        assert_eq!(marks[0]["resolution"], "unresolved");
        assert_eq!(
            marks[0]["quote"], "basic unit",
            "the original quote survives"
        );
        assert_eq!(marks[0]["createdAt"], first[0]["createdAt"]);
    }

    #[test]
    fn marks_belong_to_their_chapter() {
        let dir = temp_library();
        invoke(
            dir.path(),
            "save_annotation",
            contract()["save_annotation"].clone(),
        )
        .unwrap();
        let other = invoke(
            dir.path(),
            "list_annotations",
            json!({"chapterId": "membranes"}),
        )
        .unwrap();
        assert_eq!(other, json!([]));
    }

    #[test]
    fn delete_removes_only_the_named_mark() {
        let dir = temp_library();
        let mut args = contract()["save_annotation"].clone();
        invoke(dir.path(), "save_annotation", args.clone()).unwrap();
        args["mark"]["annotationId"] = json!("mark-2");
        invoke(dir.path(), "save_annotation", args).unwrap();

        let removed = invoke(
            dir.path(),
            "delete_annotation",
            json!({"annotationId": "mark-1"}),
        )
        .unwrap();
        assert_eq!(removed, json!(true));
        let again = invoke(
            dir.path(),
            "delete_annotation",
            json!({"annotationId": "mark-1"}),
        )
        .unwrap();
        assert_eq!(again, json!(false));

        let listed = invoke(
            dir.path(),
            "list_annotations",
            json!({"chapterId": "cells"}),
        )
        .unwrap();
        let ids: Vec<&str> = listed
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["annotationId"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["mark-2"]);
    }

    #[test]
    fn refuses_path_escaping_chapter_ids_and_empty_mark_ids() {
        let dir = temp_library();
        let escape =
            invoke(dir.path(), "list_annotations", json!({"chapterId": "../x"})).unwrap_err();
        assert!(
            escape.to_string().contains("invalid chapter id"),
            "{escape}"
        );

        let mut args = contract()["save_annotation"].clone();
        args["mark"]["annotationId"] = json!("");
        let empty = invoke(dir.path(), "save_annotation", args).unwrap_err();
        assert!(empty.to_string().contains("annotation id"), "{empty}");
    }
}
