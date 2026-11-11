//! Shelf seams: many course folders under one library root, and the one-time
//! rename of today's single-course root.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tutordocs_store::{Annotation, ChapterRef, Course, Library, Shelf, StoreError};

fn slug_root(dir: &Path) -> PathBuf {
    let root = dir.to_path_buf();
    let lib = Library::create(&root).unwrap();
    lib.write_course(&Course {
        id: "bio101".into(),
        title: "Cell Biology".into(),
        objectives: vec!["Explain the membrane".into()],
        chapters: vec![ChapterRef {
            id: "cells".into(),
            title: "Cells".into(),
            summary: String::new(),
            ..ChapterRef::default()
        }],
        ..Course::default()
    })
    .unwrap();
    fs::write(root.join("sources/notes.md"), "notes\n").unwrap();
    fs::write(root.join("chapters/cells/index.md"), "# Cells\n").unwrap();
    let mut mark = Annotation::new("a1", "chapters/cells");
    mark.quote = "bilayer".into();
    lib.put_annotation(&mark).unwrap();
    drop(lib);
    fs::write(root.join(".tutordocs/agent.json"), r#"{"driver":"claude"}"#).unwrap();
    fs::write(root.join(".DS_Store"), "stay").unwrap();
    fs::write(root.join("notes.txt"), "stray").unwrap();
    root
}

/// A second connection keeps the WAL and SHM files on disk while migration
/// renames them. The annotation is already checkpointed into `store.db`.
fn hold_wal(db: &Path) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    conn.execute("CREATE TABLE IF NOT EXISTS keep_wal (n INTEGER)", [])
        .unwrap();
    conn.execute("INSERT INTO keep_wal VALUES (1)", []).unwrap();
    conn
}

fn ino(path: &Path) -> u64 {
    fs::metadata(path).unwrap().ino()
}

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    walk(root, root, &mut out);
    out
}

#[test]
fn migrating_a_one_course_library_renames_the_database_and_its_wal_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = slug_root(tmp.path());
    let db = root.join(".tutordocs/store.db");
    let wal = root.join(".tutordocs/store.db-wal");
    let shm = root.join(".tutordocs/store.db-shm");
    let conn = hold_wal(&db);
    assert!(
        wal.is_file() && shm.is_file(),
        "the WAL pair must move with the db"
    );
    let db_ino = ino(&db);
    let wal_ino = ino(&wal);
    let shm_ino = ino(&shm);

    let shelf = Shelf::open(&root).unwrap();
    let course = root.join("courses/cell-biology");
    assert_eq!(ino(&course.join(".tutordocs/store.db")), db_ino);
    assert_eq!(ino(&course.join(".tutordocs/store.db-wal")), wal_ino);
    assert_eq!(ino(&course.join(".tutordocs/store.db-shm")), shm_ino);
    assert!(!db.exists() && !wal.exists() && !shm.exists());
    drop(conn);

    assert!(!root.join("course.json").exists());
    assert!(!root.join("sources").exists());
    assert!(!root.join("chapters").exists());
    assert_eq!(
        fs::read_to_string(root.join(".tutordocs/agent.json")).unwrap(),
        r#"{"driver":"claude"}"#
    );
    assert!(!course.join(".tutordocs/agent.json").exists());
    assert_eq!(fs::read_to_string(root.join(".DS_Store")).unwrap(), "stay");
    assert_eq!(fs::read_to_string(root.join("notes.txt")).unwrap(), "stray");
    assert_eq!(
        fs::read_to_string(course.join("sources/notes.md")).unwrap(),
        "notes\n"
    );

    let by_folder = shelf.open_course("cell-biology").unwrap();
    let by_json_id = shelf.open_course("bio101").unwrap();
    assert_eq!(by_folder.root(), by_json_id.root());
    assert_eq!(by_json_id.course().unwrap().id, "bio101");
    let mark = by_json_id.annotation("a1").unwrap().unwrap();
    assert_eq!(mark.quote, "bilayer");

    let summary = shelf.list().unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].id, "cell-biology");
    assert_eq!(summary[0].title, "Cell Biology");
    assert!(summary[0].tags.is_empty());
    assert!(summary[0].opened_at.is_none());
    assert!(summary[0].created_at > 1_700_000_000);
    assert!(summary[0].error.is_none());
    assert_eq!(summary[0].chapter_count, 1);
    assert_eq!(summary[0].written_chapter_count, 1);

    drop(by_folder);
    drop(by_json_id);
    let before = snapshot(&root);
    Shelf::open(&root).unwrap();
    assert_eq!(
        snapshot(&root),
        before,
        "opening again must not move or rewrite anything"
    );
}

#[test]
fn a_half_moved_library_finishes_on_the_next_open() {
    let tmp = tempfile::tempdir().unwrap();
    let root = slug_root(tmp.path());
    let db = root.join(".tutordocs/store.db");
    let conn = hold_wal(&db);
    let wal_ino = ino(&root.join(".tutordocs/store.db-wal"));
    let shm_ino = ino(&root.join(".tutordocs/store.db-shm"));
    let db_ino = ino(&db);
    let dest = root.join("courses/cell-biology");
    fs::create_dir_all(&dest).unwrap();
    fs::rename(root.join("course.json"), dest.join("course.json")).unwrap();
    fs::rename(root.join("sources"), dest.join("sources")).unwrap();

    Shelf::open(&root).unwrap();
    assert_eq!(ino(&dest.join(".tutordocs/store.db")), db_ino);
    assert_eq!(ino(&dest.join(".tutordocs/store.db-wal")), wal_ino);
    assert_eq!(ino(&dest.join(".tutordocs/store.db-shm")), shm_ino);
    assert!(dest.join("chapters/cells/index.md").is_file());
    assert_eq!(
        fs::read_to_string(root.join(".tutordocs/agent.json")).unwrap(),
        r#"{"driver":"claude"}"#
    );
    drop(conn);
    let lib = Library::open(&dest).unwrap();
    assert_eq!(lib.annotation("a1").unwrap().unwrap().quote, "bilayer");
}

#[test]
fn a_root_course_that_collides_with_an_existing_course_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    shelf.create_course("Cell Biology", &[]).unwrap();
    let dest_json = fs::read(tmp.path().join("courses/cell-biology/course.json")).unwrap();
    let root_json = br#"{"id":"other","title":"Cell Biology","objectives":[],"chapters":[]}"#;
    fs::write(tmp.path().join("course.json"), root_json).unwrap();

    let err = Shelf::open(tmp.path()).unwrap_err();
    assert!(err.to_string().contains("nothing was moved"), "{err}");
    assert_eq!(fs::read(tmp.path().join("course.json")).unwrap(), root_json);
    assert_eq!(
        fs::read(tmp.path().join("courses/cell-biology/course.json")).unwrap(),
        dest_json
    );
    let _ = shelf;
}

#[test]
fn an_already_migrated_shelf_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    let made = shelf.create_course("Biology", &["bio".into()]).unwrap();
    assert_eq!(made.id, "biology");
    fs::write(tmp.path().join(".DS_Store"), "stay").unwrap();
    fs::write(
        tmp.path().join(".tutordocs/agent.json"),
        r#"{"driver":"agy"}"#,
    )
    .unwrap();
    let before = snapshot(tmp.path());
    Shelf::open(tmp.path()).unwrap();
    assert_eq!(snapshot(tmp.path()), before);
    assert!(matches!(
        Shelf::open(tmp.path().join("missing")).unwrap_err(),
        StoreError::NotALibrary(_)
    ));
}

#[test]
fn a_folder_of_unrelated_files_is_not_a_library() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("notes.txt"), "mine").unwrap();
    let err = Shelf::open(tmp.path()).unwrap_err();
    assert!(err.to_string().contains("not a TutorDocs library"), "{err}");
    assert!(!tmp.path().join("course.json").exists());
    assert!(!tmp.path().join("courses").exists());
}

#[test]
fn creating_a_course_slugs_the_title_and_keeps_that_id() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    let first = shelf.create_course("  Cell Biology  ", &[]).unwrap();
    let second = shelf.create_course("cell-biology", &[]).unwrap();
    assert_eq!(first.id, "cell-biology");
    assert_eq!(first.title, "Cell Biology");
    assert_eq!(second.id, "cell-biology-2");
    assert_eq!(
        shelf
            .open_course("cell-biology")
            .unwrap()
            .course()
            .unwrap()
            .id,
        "cell-biology"
    );
    let err = shelf.create_course("   ", &[]).unwrap_err();
    assert!(err.to_string().contains("title"), "{err}");
    assert_eq!(shelf.list().unwrap().len(), 2);
}

#[test]
fn tags_are_trimmed_deduped_and_rejected_when_empty_or_too_long() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    let err = shelf.create_course("Biology", &["  ".into()]).unwrap_err();
    assert!(err.to_string().contains("empty"), "{err}");
    assert_eq!(shelf.list().unwrap().len(), 0);

    let ok = "a".repeat(40);
    let made = shelf
        .create_course(
            "Biology",
            &[" Bio ".into(), "bio".into(), "History".into(), ok.clone()],
        )
        .unwrap();
    assert_eq!(
        made.tags,
        vec!["Bio".to_string(), "History".to_string(), ok.clone()]
    );

    let too_long = "b".repeat(41);
    let err = shelf.set_tags(&made.id, &[too_long]).unwrap_err();
    assert!(err.to_string().contains("40"), "{err}");
    assert_eq!(
        shelf.list().unwrap()[0].tags,
        vec!["Bio".to_string(), "History".to_string(), ok]
    );

    let replaced = shelf.set_tags("biology", &["cells".into()]).unwrap();
    assert_eq!(replaced.tags, vec!["cells".to_string()]);
    assert_eq!(replaced.id, "biology");
}

#[test]
fn a_missing_or_escaping_course_id_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    shelf.create_course("Biology", &[]).unwrap();
    let missing = shelf.open_course("nope").unwrap_err();
    assert!(missing.to_string().contains("no course"), "{missing}");
    for bad in ["../secret", "a/b", "", ".."] {
        let err = shelf.open_course(bad).unwrap_err();
        assert!(
            matches!(err, StoreError::InvalidCourseId(_)),
            "{bad}: {err}"
        );
        assert!(!tmp.path().join("secret").exists());
    }
}

#[test]
fn an_unreadable_course_json_is_listed_with_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    let good = shelf.create_course("Biology", &["bio".into()]).unwrap();
    let bad = tmp.path().join("courses/broken");
    fs::create_dir_all(bad.join(".tutordocs")).unwrap();
    fs::write(bad.join("course.json"), "{").unwrap();

    let list = shelf.list().unwrap();
    assert_eq!(list.len(), 2);
    let broken = list.iter().find(|c| c.id == "broken").unwrap();
    assert!(broken.error.is_some(), "{broken:?}");
    let biology = list.iter().find(|c| c.id == good.id).unwrap();
    assert!(biology.error.is_none());
    assert_eq!(biology.tags, vec!["bio".to_string()]);
}

#[test]
fn chapter_counts_distinguish_written_chapters() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    let made = shelf.create_course("Biology", &[]).unwrap();
    let lib = shelf.open_course(&made.id).unwrap();
    lib.write_course(&Course {
        id: made.id.clone(),
        title: "Biology".into(),
        objectives: vec!["cells".into()],
        chapters: vec![
            ChapterRef {
                id: "cells".into(),
                title: "Cells".into(),
                summary: String::new(),
                ..ChapterRef::default()
            },
            ChapterRef {
                id: "genes".into(),
                title: "Genes".into(),
                summary: String::new(),
                ..ChapterRef::default()
            },
        ],
        ..Course::default()
    })
    .unwrap();
    fs::write(lib.root().join("chapters/cells/index.md"), "# Cells\n").unwrap();
    drop(lib);

    let summary = shelf.list().unwrap().pop().unwrap();
    assert_eq!(summary.chapter_count, 2);
    assert_eq!(summary.written_chapter_count, 1);
    assert_eq!(summary.objectives, vec!["cells".to_string()]);
    assert!(summary.error.is_none());
}

#[test]
fn the_summary_carries_chapter_titles_and_the_stored_course_id() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    let made = shelf.create_course("Biology", &[]).unwrap();
    let lib = shelf.open_course(&made.id).unwrap();
    lib.write_course(&Course {
        id: "bio101".into(),
        title: "Biology".into(),
        objectives: Vec::new(),
        chapters: vec![
            ChapterRef {
                id: "cells".into(),
                title: "Cells and membranes".into(),
                summary: String::new(),
                ..ChapterRef::default()
            },
            ChapterRef {
                id: "genes".into(),
                title: String::new(),
                summary: String::new(),
                ..ChapterRef::default()
            },
        ],
        ..Course::default()
    })
    .unwrap();
    drop(lib);

    let summary = shelf.list().unwrap().pop().unwrap();
    assert_eq!(summary.id, "biology");
    assert_eq!(summary.stored_id, "bio101");
    assert_eq!(
        summary.chapter_titles,
        vec!["Cells and membranes".to_string(), "genes".to_string()],
        "an untitled chapter reads as its id, as the sidebar shows it"
    );

    let broken = tmp.path().join("courses/broken");
    fs::create_dir_all(broken.join(".tutordocs")).unwrap();
    fs::write(broken.join("course.json"), "{").unwrap();
    let list = shelf.list().unwrap();
    let broken = list.iter().find(|c| c.id == "broken").unwrap();
    assert!(broken.chapter_titles.is_empty());
    assert_eq!(broken.stored_id, "");
}

#[test]
fn opened_at_is_recorded_without_changing_the_course_id() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    let made = shelf.create_course("Biology", &["bio".into()]).unwrap();
    assert!(made.opened_at.is_none());
    let opened = shelf.mark_opened(&made.id).unwrap();
    assert_eq!(opened.id, "biology");
    assert_eq!(opened.created_at, made.created_at);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(opened.opened_at.unwrap() <= now && opened.opened_at.unwrap() + 5 >= now);
    let retagged = shelf.set_tags("biology", &["cells".into()]).unwrap();
    assert_eq!(retagged.opened_at, opened.opened_at);
    assert_eq!(retagged.created_at, made.created_at);
    assert_eq!(retagged.id, "biology");
}

#[test]
fn two_courses_do_not_share_annotations() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    let biology = shelf.create_course("Biology", &[]).unwrap();
    let history = shelf.create_course("History", &[]).unwrap();
    shelf
        .open_course(&biology.id)
        .unwrap()
        .put_annotation(&Annotation::new("m1", "chapters/intro"))
        .unwrap();
    assert!(shelf
        .open_course(&history.id)
        .unwrap()
        .annotation("m1")
        .unwrap()
        .is_none());
    assert!(shelf
        .open_course(&biology.id)
        .unwrap()
        .annotation("m1")
        .unwrap()
        .is_some());
}

#[test]
fn the_only_unopened_course_is_the_active_one_and_a_later_open_wins() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    let biology = shelf.create_course("Biology", &[]).unwrap();
    assert_eq!(
        shelf.active_course_dir().unwrap(),
        tmp.path().join("courses").join(&biology.id)
    );
    let history = shelf.create_course("History", &[]).unwrap();
    assert!(shelf.active_course_dir().is_err());
    shelf.mark_opened(&biology.id).unwrap();
    shelf.mark_opened(&history.id).unwrap();
    assert_eq!(
        shelf.active_course_dir().unwrap(),
        tmp.path().join("courses").join(&history.id)
    );
}

#[test]
fn app_files_at_the_root_never_count_as_an_unfinished_move() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::create(tmp.path()).unwrap();
    shelf.create_course("Cell Biology", &[]).unwrap();
    shelf.create_course("Linear Algebra", &[]).unwrap();
    let meta = tmp.path().join(".tutordocs");
    fs::create_dir_all(&meta).unwrap();
    fs::write(meta.join("agent.json"), "{}").unwrap();
    fs::write(meta.join("agent.json.tmp"), "{}").unwrap();
    fs::write(meta.join(".DS_Store"), "x").unwrap();

    let reopened = Shelf::open(tmp.path()).unwrap();
    assert_eq!(reopened.list().unwrap().len(), 2);
    assert!(meta.join("agent.json.tmp").exists());
    assert!(meta.join(".DS_Store").exists());
}
