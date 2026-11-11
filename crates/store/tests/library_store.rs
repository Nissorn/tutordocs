//! Public-seam tests for the store, run against temp directories only.
//!
//! Durability tests drop the `Library` (closing the connection) and reopen the
//! same on-disk file with `Library::open`; asserting against the same connection
//! would prove nothing.

use tutordocs_store::{
    Annotation, AnnotationKind, ChapterAnswer, Library, NewAnswer, NewMasteryEvent, StoreError,
    TextSelector,
};

fn temp_lib() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().expect("temp dir");
    let lib = Library::create(dir.path()).expect("create library");
    (dir, lib)
}

#[test]
fn create_writes_the_layout() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::create(dir.path()).unwrap();
    assert!(dir.path().join("sources").is_dir());
    assert!(dir.path().join("chapters").is_dir());
    assert!(dir.path().join("course.json").is_file());
    assert!(dir.path().join(".tutordocs/store.db").is_file());
    let course = lib.course().unwrap();
    assert!(course.chapters.is_empty());
    assert_eq!(lib.courses().unwrap().len(), 1);
}

#[test]
fn create_refuses_to_clobber_existing_course() {
    let dir = tempfile::tempdir().unwrap();
    let _lib = Library::create(dir.path()).unwrap();
    let custom = r#"{"id":"mine","title":"Keep","objectives":["o"],"chapters":[]}"#;
    std::fs::write(dir.path().join("course.json"), custom).unwrap();
    let err = Library::create(dir.path()).unwrap_err();
    assert!(matches!(err, StoreError::AlreadyExists(_)));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("course.json")).unwrap(),
        custom
    );
}

#[test]
fn open_missing_course_json_is_not_a_library() {
    let dir = tempfile::tempdir().unwrap();
    let err = Library::open(dir.path()).unwrap_err();
    assert!(matches!(err, StoreError::NotALibrary(_)));
}

#[test]
fn open_reads_user_content_without_rewriting_it() {
    let (dir, lib) = temp_lib();
    drop(lib);
    let course_json = r#"{
        "id":"c1","title":"Biology","objectives":["cells","genes"],
        "chapters":[{"id":"ch1","title":"Cells"},{"id":"ch2","title":"Genes"}],
        "currentVariantId":"current"
    }"#;
    let course_path = dir.path().join("course.json");
    std::fs::write(&course_path, course_json).unwrap();
    let chapter_dir = dir.path().join("chapters/ch1");
    std::fs::create_dir_all(&chapter_dir).unwrap();
    let md = "# Cells\n\nThe membrane is a bilayer.\n";
    std::fs::write(chapter_dir.join("index.md"), md).unwrap();

    let before_course = std::fs::read(&course_path).unwrap();
    let before_md = std::fs::read(chapter_dir.join("index.md")).unwrap();
    let lib = Library::open(dir.path()).unwrap();
    let course = lib.course().unwrap();
    assert_eq!(course.id, "c1");
    assert_eq!(course.objectives, vec!["cells", "genes"]);
    assert_eq!(course.chapters.len(), 2);
    assert_eq!(std::fs::read(&course_path).unwrap(), before_course);
    assert_eq!(
        std::fs::read(chapter_dir.join("index.md")).unwrap(),
        before_md
    );
}

#[test]
fn placeholder_chapters_list_correctly() {
    let (dir, lib) = temp_lib();
    drop(lib);
    let course_json = r#"{
        "id":"c1","title":"T","objectives":[],
        "chapters":[
            {"id":"done","title":"Done"},
            {"id":"empty","title":"Empty"},
            {"id":"absent","title":"Absent"}
        ]
    }"#;
    std::fs::write(dir.path().join("course.json"), course_json).unwrap();
    // "done" has markdown nested under visuals-adjacent subdir; "empty" is a
    // placeholder directory with no markdown; "absent" has no directory at all.
    let done = dir.path().join("chapters/done");
    std::fs::create_dir_all(done.join("visuals")).unwrap();
    std::fs::write(done.join("index.md"), "# Done\n").unwrap();
    std::fs::create_dir_all(dir.path().join("chapters/empty")).unwrap();

    let lib = Library::open(dir.path()).unwrap();
    let chapters = lib.chapters().unwrap();
    assert_eq!(chapters.len(), 3);
    assert_eq!(chapters[0].id, "done");
    assert!(!chapters[0].is_placeholder);
    assert_eq!(chapters[1].id, "empty");
    assert!(chapters[1].is_placeholder, "empty dir is a placeholder");
    assert_eq!(chapters[2].id, "absent");
    assert!(chapters[2].is_placeholder, "missing dir is a placeholder");
}

#[test]
fn annotation_survives_close_and_reopen() {
    let (dir, lib) = temp_lib();
    let mut a = Annotation::new("a1", "ch1");
    a.kind = AnnotationKind::Note;
    a.quote = "the membrane is a bilayer".into();
    a.note = "remember this".into();
    a.locator = 3;
    a.selectors = vec![
        TextSelector::TextQuoteSelector {
            exact: "the membrane is a bilayer".into(),
            prefix: "Cells ".into(),
            suffix: " and".into(),
        },
        TextSelector::TextPositionSelector { start: 10, end: 35 },
    ];
    lib.put_annotation(&a).unwrap();
    drop(lib); // close the connection

    let reopened = Library::open(dir.path()).unwrap();
    let got = reopened
        .annotation("a1")
        .unwrap()
        .expect("annotation persisted");
    assert_eq!(got.annotation_id, "a1");
    assert_eq!(got.material_id, "ch1");
    assert_eq!(got.kind, AnnotationKind::Note);
    assert_eq!(got.note, "remember this");
    assert_eq!(got.locator, 3);
    assert_eq!(got.selectors.len(), 2);
    assert_eq!(reopened.annotations_for("ch1").unwrap().len(), 1);
    assert!(reopened.annotation("missing").unwrap().is_none());
}

#[test]
fn annotation_normalization_is_persisted() {
    let (_dir, lib) = temp_lib();
    let mut a = Annotation::new("a2", "ch1");
    a.color = "chartreuse".into(); // off-palette
    a.selectors = vec![TextSelector::TextQuoteSelector {
        exact: "q".repeat(2500),
        prefix: "p".repeat(300),
        suffix: "s".repeat(300),
    }];
    lib.put_annotation(&a).unwrap();
    let got = lib.annotation("a2").unwrap().unwrap();
    assert_eq!(got.color, "yellow");
    match &got.selectors[0] {
        TextSelector::TextQuoteSelector {
            exact,
            prefix,
            suffix,
        } => {
            assert_eq!(exact.chars().count(), 2000);
            assert_eq!(prefix.chars().count(), 128);
            assert_eq!(suffix.chars().count(), 128);
        }
        _ => panic!("expected quote selector"),
    }
}

#[test]
fn annotation_upsert_and_delete() {
    let (_dir, lib) = temp_lib();
    let mut a = Annotation::new("a3", "ch1");
    a.note = "first".into();
    lib.put_annotation(&a).unwrap();
    a.note = "second".into();
    lib.put_annotation(&a).unwrap();
    assert_eq!(lib.annotation("a3").unwrap().unwrap().note, "second");
    assert_eq!(lib.annotations_for("ch1").unwrap().len(), 1);
    assert!(lib.delete_annotation("a3").unwrap());
    assert!(!lib.delete_annotation("a3").unwrap());
    assert!(lib.annotation("a3").unwrap().is_none());
}

#[test]
fn mastery_event_survives_close_and_reopen() {
    let (dir, lib) = temp_lib();
    let mut e = NewMasteryEvent::new("obj-1", 1, "mastery.assessed", 100.0);
    e.payload_json = r#"{"result":"correct"}"#.into();
    e.session_id = "s1".into();
    e.turn_id = "t1".into();
    let id1 = lib.append_mastery_event(&e).unwrap();
    let e2 = NewMasteryEvent::new("obj-1", 2, "path.saved", 200.0);
    let id2 = lib.append_mastery_event(&e2).unwrap();
    assert!(id2 > id1, "ids are monotone");
    drop(lib);

    let reopened = Library::open(dir.path()).unwrap();
    let events = reopened.mastery_events("obj-1").unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].id, id1);
    assert_eq!(events[0].event_type, "mastery.assessed");
    assert_eq!(events[0].session_id, "s1");
    assert_eq!(events[0].turn_id, "t1");
    assert_eq!(events[0].payload_json, r#"{"result":"correct"}"#);
    assert_eq!(events[1].id, id2);
    assert_eq!(events[1].payload_json, "{}");
}

#[test]
fn a_failed_second_mastery_insert_leaves_no_new_events() {
    let (_dir, lib) = temp_lib();
    let first = NewMasteryEvent::new("obj-1", 1, "mastery.assessed", 1.0);
    let second = NewMasteryEvent::new("obj-1", 2, " ", 2.0);

    let err = lib.append_mastery_events(&[first, second]).unwrap_err();

    assert!(matches!(err, StoreError::InvalidEvent(_)), "{err:?}");
    assert!(
        lib.mastery_events("obj-1").unwrap().is_empty(),
        "the first insert must roll back with the second"
    );
}

#[test]
fn empty_event_type_is_rejected() {
    let (_dir, lib) = temp_lib();
    let e = NewMasteryEvent::new("obj-1", 1, "  ", 1.0);
    assert!(lib.append_mastery_event(&e).is_err());
    assert!(lib.mastery_events("obj-1").unwrap().is_empty());
}

#[test]
fn repetition_state_is_derived_and_survives_reopen() {
    let (dir, lib) = temp_lib();
    for (i, result) in ["correct", "correct", "incorrect"].iter().enumerate() {
        let mut e = NewMasteryEvent::new("obj-1", 1, "mastery.assessed", (i as f64) * 86_400.0);
        e.payload_json = format!("{{\"result\":\"{result}\"}}");
        lib.append_mastery_event(&e).unwrap();
    }
    let now = 3.0 * 86_400.0;
    let before = lib.repetition_state("obj-1", now).unwrap();
    assert_eq!(before.review_count, 3);
    assert_eq!(before.lapse_count, 1);
    drop(lib);

    let reopened = Library::open(dir.path()).unwrap();
    let after = reopened.repetition_state("obj-1", now).unwrap();
    assert_eq!(before, after, "derived state is identical after reopen");
}

#[test]
fn schema_has_eight_event_columns_and_no_search_index() {
    let (dir, lib) = temp_lib();
    // Inspect the on-disk schema through a fresh raw connection.
    drop(lib);
    let conn = rusqlite::Connection::open(dir.path().join(".tutordocs/store.db")).unwrap();

    let mut stmt = conn.prepare("PRAGMA table_info(mastery_events)").unwrap();
    let cols: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        cols,
        vec![
            "id",
            "path_id",
            "revision",
            "event_type",
            "payload_json",
            "session_id",
            "turn_id",
            "created_at",
        ],
        "the eight verified columns, in order"
    );

    let mut stmt = conn
        .prepare("SELECT name, type FROM sqlite_master WHERE type IN ('table','index')")
        .unwrap();
    let objects: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    for (name, _) in &objects {
        let lower = name.to_lowercase();
        assert!(
            !lower.contains("fts")
                && !lower.contains("vector")
                && !lower.contains("embed")
                && !lower.contains("sync"),
            "decision 10 forbids search/sync machinery, found {name}"
        );
    }
    let tables: Vec<&str> = objects
        .iter()
        .filter(|(_, t)| t == "table")
        .map(|(n, _)| n.as_str())
        .collect();
    assert!(tables.contains(&"annotations"));
    assert!(tables.contains(&"mastery_events"));
    assert!(
        !tables.contains(&"mastery_paths") && !tables.contains(&"repetition_state"),
        "no mutable mastery snapshot or stored repetition state"
    );
    let indexes: Vec<&str> = objects
        .iter()
        .filter(|(_, t)| t == "index")
        .map(|(n, _)| n.as_str())
        .collect();
    assert!(indexes.contains(&"idx_mastery_events_path_revision"));
}

#[test]
fn answers_persist_per_chapter_newest_first_and_refuse_bad_chapter_ids() {
    let (dir, lib) = temp_lib();
    let first = lib
        .save_answer(
            "ch1",
            &NewAnswer {
                quote: Some("the membrane is a bilayer".into()),
                question: "Why a bilayer?".into(),
                answer: "Hydrophobic tails face inward.".into(),
                sources: vec!["lecture.pdf".into(), "notes.md".into()],
                asked_at: "2026-11-09T10:00:00Z".into(),
            },
        )
        .unwrap();
    let second = lib
        .save_answer(
            "ch1",
            &NewAnswer {
                quote: None,
                question: "What does this chapter cover?".into(),
                answer: "Cell membranes.".into(),
                sources: Vec::new(),
                asked_at: "2026-11-09T10:05:00Z".into(),
            },
        )
        .unwrap();
    lib.save_answer(
        "ch2",
        &NewAnswer {
            quote: None,
            question: "Other chapter?".into(),
            answer: "Genes.".into(),
            sources: vec!["genes.pdf".into()],
            asked_at: "2026-11-09T10:10:00Z".into(),
        },
    )
    .unwrap();
    assert!(second.id > first.id);
    drop(lib);

    let reopened = Library::open(dir.path()).unwrap();
    let listed = reopened.list_answers("ch1").unwrap();
    assert_eq!(
        listed,
        vec![
            ChapterAnswer {
                id: second.id,
                quote: None,
                question: "What does this chapter cover?".into(),
                answer: "Cell membranes.".into(),
                sources: Vec::new(),
                asked_at: "2026-11-09T10:05:00Z".into(),
            },
            ChapterAnswer {
                id: first.id,
                quote: Some("the membrane is a bilayer".into()),
                question: "Why a bilayer?".into(),
                answer: "Hydrophobic tails face inward.".into(),
                sources: vec!["lecture.pdf".into(), "notes.md".into()],
                asked_at: "2026-11-09T10:00:00Z".into(),
            },
        ]
    );
    assert_eq!(reopened.list_answers("ch2").unwrap().len(), 1);
    assert!(reopened.list_answers("ch3").unwrap().is_empty());
    assert!(matches!(
        reopened.list_answers("../escape"),
        Err(StoreError::InvalidChapterId(_))
    ));
}
