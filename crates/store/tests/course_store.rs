//! Writing an outline and reading a chapter's markdown through the public
//! seam, the two store calls lazy generation (ticket 07) builds on.

use tutordocs_store::{
    ChapterRef, Course, Library, Outline, OutlineChapter, OutlineStatus, StoreError,
};

fn temp_lib() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().expect("temp dir");
    let lib = Library::create(dir.path()).expect("create library");
    (dir, lib)
}

fn chapter(id: &str, title: &str, summary: &str) -> ChapterRef {
    ChapterRef {
        id: id.to_string(),
        title: title.to_string(),
        summary: summary.to_string(),
        ..ChapterRef::default()
    }
}

fn outline() -> Course {
    Course {
        id: "rust".to_string(),
        title: "Rust ownership".to_string(),
        objectives: vec!["Explain moves".to_string()],
        chapters: vec![
            chapter("moves", "Moves", "What a move does"),
            chapter("borrows", "Borrows", "Shared and unique borrows"),
        ],
        outline_status: OutlineStatus::Approved,
        ..Default::default()
    }
}

#[test]
fn a_written_outline_reads_back_with_every_chapter_a_placeholder_directory() {
    let (dir, lib) = temp_lib();

    lib.write_course(&outline()).unwrap();

    let reopened = Library::open(dir.path()).unwrap();
    let course = reopened.course().unwrap();
    assert_eq!(course.id, "rust");
    assert_eq!(course.title, "Rust ownership");
    assert_eq!(course.objectives, vec!["Explain moves"]);
    assert_eq!(course.chapters, outline().chapters);
    assert_eq!(course.outline_status, OutlineStatus::Approved);
    assert_eq!(course.current_variant_id.as_deref(), Some("current"));
    let chapters = reopened.chapters().unwrap();
    let ids: Vec<&str> = chapters.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec!["moves", "borrows"]);
    assert!(chapters.iter().all(|c| c.is_placeholder));
    assert!(dir.path().join("chapters/moves").is_dir());
    assert!(dir.path().join("chapters/borrows").is_dir());
}

#[test]
fn a_new_outline_keeps_a_chapter_that_is_already_written() {
    let (dir, lib) = temp_lib();
    std::fs::create_dir_all(dir.path().join("chapters/moves")).unwrap();
    std::fs::write(dir.path().join("chapters/moves/index.md"), "# Moves\n").unwrap();

    lib.write_course(&outline()).unwrap();

    let chapters = lib.chapters().unwrap();
    assert!(!chapters[0].is_placeholder);
    assert_eq!(lib.chapter_markdown("moves").unwrap().unwrap(), "# Moves\n");
}

#[test]
fn a_chapter_summary_is_optional_in_course_json() {
    let (dir, lib) = temp_lib();
    let json = r#"{"id":"c","title":"T","objectives":[],"chapters":[{"id":"a","title":"A"}]}"#;
    std::fs::write(dir.path().join("course.json"), json).unwrap();

    assert_eq!(lib.course().unwrap().chapters, vec![chapter("a", "A", "")]);
}

#[test]
fn an_outline_whose_chapter_id_leaves_chapters_is_refused_and_nothing_changes() {
    let (dir, lib) = temp_lib();
    let before = std::fs::read_to_string(dir.path().join("course.json")).unwrap();
    let mut bad = outline();
    bad.chapters.push(chapter("../escape", "Escape", ""));

    let err = lib.write_course(&bad).unwrap_err();

    assert!(matches!(err, StoreError::InvalidChapterId(_)), "{err:?}");
    let after = std::fs::read_to_string(dir.path().join("course.json")).unwrap();
    assert_eq!(after, before);
    assert!(!dir.path().join("escape").exists());
    assert!(!dir.path().join("chapters/moves").exists());
}

#[test]
fn chapter_markdown_prefers_index_md_and_is_none_for_a_placeholder() {
    let (dir, lib) = temp_lib();
    lib.write_course(&outline()).unwrap();
    let moves = dir.path().join("chapters/moves");
    std::fs::write(moves.join("a-notes.md"), "notes\n").unwrap();
    std::fs::write(moves.join("index.md"), "# Moves\n").unwrap();

    assert_eq!(lib.chapter_markdown("moves").unwrap().unwrap(), "# Moves\n");
    assert_eq!(lib.chapter_markdown("borrows").unwrap(), None);
    assert!(matches!(
        lib.chapter_markdown("../moves").unwrap_err(),
        StoreError::InvalidChapterId(_)
    ));
}

#[test]
fn a_missing_outline_status_reads_as_approved_and_an_existing_course_still_opens() {
    let (dir, _lib) = temp_lib();
    let legacy = r#"{"id":"rust","title":"Rust ownership","objectives":["Explain moves"],"chapters":[{"id":"moves","title":"Moves","summary":"What a move does"}]}"#;
    std::fs::write(dir.path().join("course.json"), legacy).unwrap();
    std::fs::create_dir_all(dir.path().join("chapters/moves")).unwrap();
    std::fs::write(dir.path().join("chapters/moves/index.md"), "# Moves\n").unwrap();

    let reopened = Library::open(dir.path()).unwrap();
    let course = reopened.course().unwrap();
    assert_eq!(course.outline_status, OutlineStatus::Approved);
    assert_eq!(
        reopened.chapter_markdown("moves").unwrap().as_deref(),
        Some("# Moves\n")
    );

    let mut draft = course;
    draft.outline_status = OutlineStatus::Draft;
    reopened.write_course(&draft).unwrap();
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("course.json")).unwrap())
            .unwrap();
    assert_eq!(raw["outlineStatus"], "draft");
    assert_eq!(
        Library::open(dir.path())
            .unwrap()
            .course()
            .unwrap()
            .outline_status,
        OutlineStatus::Draft
    );
}

#[test]
fn save_outline_adds_removes_reorders_and_renames_chapters_and_saves_comments() {
    let (dir, lib) = temp_lib();
    let mut initial = outline();
    initial.outline_status = OutlineStatus::Draft;
    lib.write_course(&initial).unwrap();
    std::fs::write(dir.path().join("chapters/moves/index.md"), "# Moves\n").unwrap();

    let got = lib.outline().unwrap();
    assert_eq!(got.status, OutlineStatus::Draft);
    assert_eq!(got.chapters.len(), 2);
    assert!(got.chapters[0].written);
    assert!(!got.chapters[1].written);

    // Remove unwritten "borrows", add "Lifetimes & Slices", reorder added before "moves",
    // rename "moves" and add a comment and sources.
    let edited = Outline {
        status: OutlineStatus::Draft,
        title: "Rust Ownership Deep Dive".to_string(),
        objectives: vec![
            "Explain moves".to_string(),
            "Reason about lifetimes".to_string(),
        ],
        chapters: vec![
            OutlineChapter {
                id: None,
                title: "Lifetimes & Slices".to_string(),
                summary: "Borrowing slices".to_string(),
                sources: vec!["notes.md".to_string()],
                comment: "Include string slices".to_string(),
                written: false,
            },
            OutlineChapter {
                id: Some("moves".to_string()),
                title: "Move Semantics".to_string(),
                summary: "How ownership transfers".to_string(),
                sources: vec!["lecture-1.md".to_string()],
                comment: "Keep the diagram".to_string(),
                written: false, // UI read-only field ignored; on-disk truth wins
            },
        ],
    };

    let saved = lib.save_outline(&edited).unwrap();
    assert_eq!(saved.title, "Rust Ownership Deep Dive");
    assert_eq!(
        saved.objectives,
        vec!["Explain moves", "Reason about lifetimes"]
    );
    assert_eq!(saved.chapters.len(), 2);
    assert_eq!(
        saved.chapters[0].id.as_deref(),
        Some("lifetimes-slices"),
        "added chapter receives a slug id"
    );
    assert!(!saved.chapters[0].written);
    assert_eq!(saved.chapters[0].comment, "Include string slices");
    assert_eq!(saved.chapters[0].sources, vec!["notes.md"]);

    assert_eq!(saved.chapters[1].id.as_deref(), Some("moves"));
    assert_eq!(saved.chapters[1].title, "Move Semantics");
    assert_eq!(saved.chapters[1].summary, "How ownership transfers");
    assert_eq!(saved.chapters[1].comment, "Keep the diagram");
    assert_eq!(saved.chapters[1].sources, vec!["lecture-1.md"]);
    assert!(saved.chapters[1].written);

    assert_eq!(lib.outline().unwrap(), saved);
}

#[test]
fn save_outline_refuses_removing_a_written_chapter_and_refuses_an_unknown_id() {
    let (dir, lib) = temp_lib();
    lib.write_course(&outline()).unwrap();
    std::fs::write(dir.path().join("chapters/moves/index.md"), "# Moves\n").unwrap();
    let before = lib.outline().unwrap();

    // Try removing written chapter "moves"
    let drop_written = Outline {
        status: OutlineStatus::Approved,
        title: "Rust ownership".to_string(),
        objectives: vec!["Explain moves".to_string()],
        chapters: vec![before.chapters[1].clone()],
    };
    let err = lib.save_outline(&drop_written).unwrap_err().to_string();
    assert!(err.contains("written"), "{err}");
    assert_eq!(lib.outline().unwrap(), before);

    // Try saving an unknown chapter id
    let mut unknown = before.clone();
    unknown.chapters.push(OutlineChapter {
        id: Some("ghost".to_string()),
        title: "Ghost".to_string(),
        summary: String::new(),
        sources: Vec::new(),
        comment: String::new(),
        written: false,
    });
    let err = lib.save_outline(&unknown).unwrap_err().to_string();
    assert!(err.contains("ghost"), "{err}");
    assert_eq!(lib.outline().unwrap(), before);
}

#[test]
fn saving_the_same_outline_twice_gives_the_same_result() {
    let (_dir, lib) = temp_lib();
    lib.write_course(&outline()).unwrap();

    let mut input = lib.outline().unwrap();
    input.chapters.push(OutlineChapter {
        id: None,
        title: "Pinning".to_string(),
        summary: "Self-referential types".to_string(),
        sources: Vec::new(),
        comment: "Keep it brief".to_string(),
        written: false,
    });

    let first = lib.save_outline(&input).unwrap();
    let second_from_saved = lib.save_outline(&first).unwrap();
    assert_eq!(first, second_from_saved);
    let second_from_input = lib.save_outline(&input).unwrap();
    assert_eq!(first, second_from_input);
}
