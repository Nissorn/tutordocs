//! PDF sources: list the PDFs in `sources/`, return one as page text with
//! word anchors, and keep its marks.
//!
//! The reader lays each page out as the extracted text at its extracted rects,
//! so a highlight rect and the words on screen share one geometry. Marks reuse
//! the chapter `Mark` under the material `sources/<file name>`, with the page
//! as `locator` and normalized rects in `rects`.
//!
//! `read_text_source` is the same door for the other readable kinds: a
//! markdown or text source comes back as plain text, read-only.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;
use tutordocs_ingest::pdf::{extract_pdf, PageCaveat, PageText, GLYPH_HEIGHT_FACTOR};

use crate::annotations::{list_marks, save_mark, Mark, MarkInput, Rect};
use crate::reader::{is_plain_component, ReaderState};

/// A PDF as the document column needs it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfSource {
    pub name: String,
    /// Ticket 03's em-to-glyph-box ratio, so the UI sizes text from a rect
    /// height with the same constant that produced it.
    pub glyph_height_factor: f64,
    pub pages: Vec<PdfPage>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfPage {
    pub page: u32,
    pub width: f64,
    pub height: f64,
    pub text: String,
    pub caveat: Option<Caveat>,
    pub words: Vec<PdfWord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Caveat {
    TransformedFormText,
}

#[derive(Debug, Serialize)]
pub struct PdfWord {
    pub quote: String,
    pub rect: Rect,
}

fn is_pdf_name(name: &str) -> bool {
    !name.starts_with('.')
        && Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// The readable non-PDF kinds. Wire values match `Source.kind` in the contract.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TextKind {
    Markdown,
    Text,
}

/// A markdown or text source as the reader shows it: read-only text.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSource {
    pub name: String,
    pub kind: TextKind,
    pub text: String,
}

fn text_kind(name: &str) -> Option<TextKind> {
    if name.starts_with('.') {
        return None;
    }
    let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" => Some(TextKind::Markdown),
        "txt" => Some(TextKind::Text),
        _ => None,
    }
}

/// A source name is one plain component that `list_pdf_sources` would show.
fn checked_name(name: &str) -> Result<&str, String> {
    if is_plain_component(name) && is_pdf_name(name) {
        Ok(name)
    } else {
        Err(format!("not a PDF source: {name:?}"))
    }
}

fn material_id(source_name: &str) -> Result<String, String> {
    Ok(format!("sources/{}", checked_name(source_name)?))
}

fn sources_dir(state: &ReaderState, course_id: &str) -> Result<PathBuf, String> {
    state.with_course(course_id, |library| Ok(library.root().join("sources")))
}

#[tauri::command]
pub fn list_pdf_sources(
    state: State<'_, ReaderState>,
    course_id: String,
) -> Result<Vec<String>, String> {
    let dir = sources_dir(&state, &course_id)?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("could not list {}: {e}", dir.display())),
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| is_pdf_name(name))
        .collect();
    names.sort();
    Ok(names)
}

/// Async so a long extraction runs off the main thread and the window stays
/// responsive. The library lock is held only to resolve the path.
#[tauri::command]
pub async fn read_pdf_source(
    state: State<'_, ReaderState>,
    course_id: String,
    name: String,
) -> Result<PdfSource, String> {
    let path = sources_dir(&state, &course_id)?.join(checked_name(&name)?);
    let bytes = std::fs::read(&path).map_err(|e| format!("could not read {name:?}: {e}"))?;
    let pages = extract_pdf(&bytes).map_err(|e| format!("{name:?}: {e}"))?;
    Ok(PdfSource {
        name,
        glyph_height_factor: GLYPH_HEIGHT_FACTOR,
        pages: pages.into_iter().map(PdfPage::from).collect(),
    })
}

/// A markdown or text file in `sources/`, by plain file name, with the same
/// name checks as the PDF path. Anything else is refused by name.
#[tauri::command]
pub fn read_text_source(
    state: State<'_, ReaderState>,
    course_id: String,
    name: String,
) -> Result<TextSource, String> {
    let kind = match text_kind(&name) {
        Some(kind) if is_plain_component(&name) => kind,
        _ => return Err(format!("not a markdown or text source: {name:?}")),
    };
    let path = sources_dir(&state, &course_id)?.join(&name);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("could not read {name:?}: {e}"))?;
    Ok(TextSource { name, kind, text })
}

impl From<PageText> for PdfPage {
    fn from(p: PageText) -> Self {
        Self {
            page: p.page,
            width: p.width,
            height: p.height,
            text: p.text,
            caveat: p.caveat.map(|c| match c {
                PageCaveat::TransformedFormText => Caveat::TransformedFormText,
            }),
            words: p
                .words
                .into_iter()
                .map(|w| PdfWord {
                    quote: w.quote,
                    rect: Rect {
                        x: w.normalized_rect.x,
                        y: w.normalized_rect.y,
                        w: w.normalized_rect.w,
                        h: w.normalized_rect.h,
                    },
                })
                .collect(),
        }
    }
}

#[tauri::command]
pub fn list_source_annotations(
    state: State<'_, ReaderState>,
    source_name: String,
) -> Result<Vec<Mark>, String> {
    list_marks(&state, &material_id(&source_name)?)
}

#[tauri::command]
pub fn save_source_annotation(
    state: State<'_, ReaderState>,
    source_name: String,
    mark: MarkInput,
) -> Result<Mark, String> {
    save_mark(&state, material_id(&source_name)?, mark)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use tutordocs_ingest::fixture::{Form, Page};

    use crate::annotations::tests::invoke;

    fn contract() -> Value {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ui/src/fixtures/pdf-source-contract.json"
        ));
        serde_json::from_str(text).expect("contract fixture is valid JSON")
    }

    /// Ticket 03's fixture page, then the same word drawn through a form at
    /// half scale, which ingest flags.
    fn lecture_pdf() -> Vec<u8> {
        let forms = [Form {
            name: "Fm1",
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            content: "BT /F1 24 Tf 72 720 Td (Hello) Tj ET",
        }];
        tutordocs_ingest::fixture::pdf(&[
            Page::text("BT /F1 24 Tf 72 720 Td (Hello world) Tj ET"),
            Page {
                content: "q 0.5 0 0 0.5 0 0 cm /Fm1 Do Q",
                forms: &forms,
            },
        ])
    }

    /// Folder id of `temp_library`. The title "Cell Biology" migrates to this slug.
    const COURSE_ID: &str = "cell-biology";

    fn temp_library() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let course = json!({
            "id": "bio101", "title": "Cell Biology", "objectives": [],
            "chapters": [{"id": "cells", "title": "Cells"}]
        });
        std::fs::write(dir.path().join("course.json"), course.to_string()).unwrap();
        let sources = dir.path().join("sources");
        std::fs::create_dir_all(&sources).unwrap();
        std::fs::write(sources.join("lecture.pdf"), lecture_pdf()).unwrap();
        std::fs::write(sources.join("notes.md"), "# Notes\n").unwrap();
        std::fs::write(sources.join("syllabus.txt"), "Week 1\n  Cells\n").unwrap();
        std::fs::write(sources.join(".hidden.md"), "secret").unwrap();
        std::fs::write(sources.join(".hidden.pdf"), lecture_pdf()).unwrap();
        dir
    }

    /// Equal as JSON, with numbers compared to 1e-9 so the extractor's f32
    /// operand rounding does not fail a hand-computed literal.
    fn assert_json_close(got: &Value, want: &Value, path: &str) {
        match (got, want) {
            (Value::Number(g), Value::Number(w)) => {
                let (g, w) = (g.as_f64().unwrap(), w.as_f64().unwrap());
                assert!((g - w).abs() < 1e-9, "{path}: got {g} want {w}");
            }
            (Value::Array(g), Value::Array(w)) => {
                assert_eq!(g.len(), w.len(), "{path}: length");
                for (i, (g, w)) in g.iter().zip(w).enumerate() {
                    assert_json_close(g, w, &format!("{path}[{i}]"));
                }
            }
            (Value::Object(g), Value::Object(w)) => {
                let keys = |o: &serde_json::Map<String, Value>| {
                    let mut k: Vec<String> = o.keys().cloned().collect();
                    k.sort();
                    k
                };
                assert_eq!(keys(g), keys(w), "{path}: keys");
                for (k, w) in w {
                    assert_json_close(&g[k], w, &format!("{path}.{k}"));
                }
            }
            _ => assert_eq!(got, want, "{path}"),
        }
    }

    #[test]
    fn lists_only_the_visible_pdfs_in_sources() {
        let dir = temp_library();
        let listed = invoke(
            dir.path(),
            "list_pdf_sources",
            json!({"courseId": COURSE_ID}),
        )
        .unwrap();
        assert_eq!(listed, contract()["list_pdf_sources"]);
    }

    #[test]
    fn reads_page_text_and_word_anchors_with_hand_computed_rects() {
        let dir = temp_library();
        let doc = invoke(
            dir.path(),
            "read_pdf_source",
            json!({"courseId": COURSE_ID, "name": "lecture.pdf"}),
        )
        .expect("the fixture PDF reads");
        assert_json_close(&doc, &contract()["read_pdf_source"], "doc");
    }

    #[test]
    fn refuses_names_outside_the_pdf_sources() {
        let dir = temp_library();
        for name in ["../course.json", "notes.md", ".hidden.pdf", "missing.pdf"] {
            let err = invoke(
                dir.path(),
                "read_pdf_source",
                json!({ "courseId": COURSE_ID, "name": name }),
            )
            .unwrap_err();
            assert!(err.to_string().contains(name), "{name}: {err}");
        }
    }

    #[test]
    fn reads_a_markdown_or_text_source_as_its_text_with_its_kind() {
        let dir = temp_library();
        let md = invoke(
            dir.path(),
            "read_text_source",
            json!({"courseId": COURSE_ID, "name": "notes.md"}),
        )
        .unwrap();
        assert_eq!(
            md,
            json!({"name": "notes.md", "kind": "markdown", "text": "# Notes\n"})
        );
        let txt = invoke(
            dir.path(),
            "read_text_source",
            json!({"courseId": COURSE_ID, "name": "syllabus.txt"}),
        )
        .unwrap();
        assert_eq!(
            txt,
            json!({"name": "syllabus.txt", "kind": "text", "text": "Week 1\n  Cells\n"})
        );
    }

    #[test]
    fn refuses_text_source_names_that_are_not_a_visible_markdown_or_text_file_in_sources() {
        let dir = temp_library();
        for name in [
            "../course.json",
            "sub/notes.md",
            "lecture.pdf",
            ".hidden.md",
            "missing.md",
        ] {
            let err = invoke(
                dir.path(),
                "read_text_source",
                json!({ "courseId": COURSE_ID, "name": name }),
            )
            .unwrap_err();
            assert!(err.to_string().contains(name), "{name}: {err}");
        }
    }

    #[test]
    fn a_source_mark_keeps_its_page_and_rects_after_the_database_is_reopened() {
        let dir = temp_library();
        let args = contract()["save_source_annotation"].clone();
        invoke(dir.path(), "save_source_annotation", args).expect("save succeeds");
        let mut listed = invoke(
            dir.path(),
            "list_source_annotations",
            json!({"sourceName": "lecture.pdf"}),
        )
        .unwrap();
        for mark in listed.as_array_mut().unwrap() {
            mark["createdAt"] = json!(1_700_000_000);
            mark["updatedAt"] = json!(1_700_000_000);
        }
        assert_json_close(&listed, &contract()["list_source_annotations"], "marks");
    }

    #[test]
    fn source_marks_and_chapter_marks_never_share_a_material() {
        let dir = temp_library();
        let mut args = contract()["save_source_annotation"].clone();
        args["sourceName"] = json!("cells.pdf");
        std::fs::write(dir.path().join("sources/cells.pdf"), lecture_pdf()).unwrap();
        invoke(dir.path(), "save_source_annotation", args).unwrap();
        let chapter = invoke(
            dir.path(),
            "list_annotations",
            json!({"chapterId": "cells"}),
        )
        .unwrap();
        let chapter_named_like_it = invoke(
            dir.path(),
            "list_annotations",
            json!({"chapterId": "cells.pdf"}),
        )
        .unwrap();
        assert_eq!((chapter, chapter_named_like_it), (json!([]), json!([])));
        let escape = invoke(
            dir.path(),
            "list_source_annotations",
            json!({"sourceName": "../x.pdf"}),
        )
        .unwrap_err();
        assert!(escape.to_string().contains("../x.pdf"), "{escape}");
    }

    /// Two courses hold the same file names. History is the course marked
    /// opened, so a command that follows the active course would return
    /// history's file. Biology's copies differ in content and in which PDFs
    /// exist.
    fn two_courses_with_same_named_sources() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        invoke(dir.path(), "create_course", json!({"title": "Biology"})).unwrap();
        invoke(dir.path(), "create_course", json!({"title": "History"})).unwrap();
        for (course, note, word) in [
            ("biology", "# Biology notes\n", "Biology"),
            ("history", "# History notes\n", "History"),
        ] {
            let sources = dir.path().join("courses").join(course).join("sources");
            std::fs::create_dir_all(&sources).unwrap();
            std::fs::write(sources.join("notes.md"), note).unwrap();
            let pdf = tutordocs_ingest::fixture::pdf(&[Page::text(&format!(
                "BT /F1 24 Tf 72 720 Td ({word}) Tj ET"
            ))]);
            std::fs::write(sources.join("lecture.pdf"), pdf).unwrap();
        }
        let biology = dir.path().join("courses/biology/sources");
        std::fs::write(biology.join("extra.pdf"), lecture_pdf()).unwrap();
        invoke(
            dir.path(),
            "mark_course_opened",
            json!({"courseId": "history"}),
        )
        .unwrap();
        dir
    }

    #[test]
    fn a_non_active_course_reads_its_own_same_named_text_source() {
        let dir = two_courses_with_same_named_sources();
        let read = invoke(
            dir.path(),
            "read_text_source",
            json!({"courseId": "biology", "name": "notes.md"}),
        )
        .expect("the non-active course's source reads");
        assert_eq!(read["text"], "# Biology notes\n");
        assert_eq!(read["name"], "notes.md");
        assert_eq!(read["kind"], "markdown");
    }

    #[test]
    fn a_non_active_course_reads_its_own_same_named_pdf() {
        let dir = two_courses_with_same_named_sources();
        let read = invoke(
            dir.path(),
            "read_pdf_source",
            json!({"courseId": "biology", "name": "lecture.pdf"}),
        )
        .expect("the non-active course's PDF reads");
        assert_eq!(read["name"], "lecture.pdf");
        assert_eq!(read["pages"][0]["text"], "Biology");
    }

    #[test]
    fn a_non_active_course_lists_its_own_pdfs() {
        let dir = two_courses_with_same_named_sources();
        let listed = invoke(
            dir.path(),
            "list_pdf_sources",
            json!({"courseId": "biology"}),
        )
        .expect("the non-active course's PDFs list");
        assert_eq!(listed, json!(["extra.pdf", "lecture.pdf"]));
    }
}
