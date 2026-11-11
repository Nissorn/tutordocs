//! Tests for course trash, list, restore and empty.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use tutordocs_store::{Annotation, Course, Library, Shelf, StoreError};

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

#[test]
fn trash_and_restore_round_trip_preserves_every_byte_and_inode() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::open_or_create(tmp.path()).unwrap();
    let summary = shelf.create_course("Cell Membranes", &[]).unwrap();
    let course_id = summary.id;
    let course_dir = shelf.course_dir(&course_id).unwrap();

    fs::write(course_dir.join("sources/notes.md"), "bilayer notes\n").unwrap();
    fs::create_dir_all(course_dir.join("chapters/cells")).unwrap();
    fs::write(
        course_dir.join("chapters/cells/index.md"),
        "# Cells\n\nContent here.\n",
    )
    .unwrap();

    let lib = Library::open(&course_dir).unwrap();
    let mut mark = Annotation::new("mark-1", "chapters/cells");
    mark.quote = "bilayer".into();
    lib.put_annotation(&mark).unwrap();
    drop(lib);

    let db_path = course_dir.join(".tutordocs/store.db");
    let _held = hold_wal(&db_path);
    let wal_path = course_dir.join(".tutordocs/store.db-wal");
    let shm_path = course_dir.join(".tutordocs/store.db-shm");
    assert!(wal_path.is_file(), "WAL file should exist");
    assert!(shm_path.is_file(), "SHM file should exist");

    let db_bytes = fs::read(&db_path).unwrap();
    let notes_bytes = fs::read(course_dir.join("sources/notes.md")).unwrap();
    let ch_bytes = fs::read(course_dir.join("chapters/cells/index.md")).unwrap();

    let db_ino = ino(&db_path);
    let wal_ino = ino(&wal_path);
    let shm_ino = ino(&shm_path);
    let note_ino = ino(&course_dir.join("sources/notes.md"));
    let ch_ino = ino(&course_dir.join("chapters/cells/index.md"));

    let entry = shelf.trash_course(&course_id).unwrap();
    assert_eq!(entry.course_id, course_id);
    assert_eq!(entry.title, "Cell Membranes");
    assert!(entry.trash_id.starts_with(&format!("{course_id}-")));
    assert!(entry.bytes > 0);
    assert!(!course_dir.exists(), "course dir must be moved out");

    let trash_list = shelf.list_trash().unwrap();
    assert_eq!(trash_list.len(), 1);
    assert_eq!(trash_list[0], entry);

    let restored = shelf.restore_course(&entry.trash_id).unwrap();
    assert_eq!(restored.id, course_id);
    assert_eq!(restored.title, "Cell Membranes");
    assert!(course_dir.exists(), "course dir must be restored");

    assert_eq!(shelf.list_trash().unwrap().len(), 0);

    // Byte-identity of sources, chapters, and store.db
    assert_eq!(fs::read(&db_path).unwrap(), db_bytes);
    assert_eq!(
        fs::read(course_dir.join("sources/notes.md")).unwrap(),
        notes_bytes
    );
    assert_eq!(
        fs::read(course_dir.join("chapters/cells/index.md")).unwrap(),
        ch_bytes
    );

    // Inode identity (renames preserved everything, including WAL and SHM)
    assert_eq!(ino(&db_path), db_ino);
    assert_eq!(ino(&wal_path), wal_ino);
    assert_eq!(ino(&shm_path), shm_ino);
    assert_eq!(ino(&course_dir.join("sources/notes.md")), note_ino);
    assert_eq!(ino(&course_dir.join("chapters/cells/index.md")), ch_ino);
}

#[test]
fn second_trash_of_recreated_course_id_gets_new_trash_id_and_keeps_both() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::open_or_create(tmp.path()).unwrap();
    let first = shelf.create_course("Biology", &[]).unwrap();
    assert_eq!(first.id, "biology");

    let entry1 = shelf.trash_course("biology").unwrap();
    assert_eq!(entry1.course_id, "biology");

    // Recreate course with same folder name "biology"
    let course_dir = tmp.path().join("courses/biology");
    let lib = Library::create(&course_dir).unwrap();
    lib.write_course(&Course {
        id: "bio-2".into(),
        title: "Biology V2".into(),
        ..Course::default()
    })
    .unwrap();
    drop(lib);

    let entry2 = shelf.trash_course("biology").unwrap();
    assert_eq!(entry2.course_id, "biology");
    assert_eq!(entry2.title, "Biology V2");
    assert_ne!(entry1.trash_id, entry2.trash_id);

    let list = shelf.list_trash().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].trash_id, entry2.trash_id);
    assert_eq!(list[1].trash_id, entry1.trash_id);
}

#[test]
fn restore_refuses_when_course_id_already_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::open_or_create(tmp.path()).unwrap();
    shelf.create_course("Biology", &[]).unwrap();

    let entry = shelf.trash_course("biology").unwrap();

    // Recreate courses/biology
    let course_dir = tmp.path().join("courses/biology");
    Library::create(&course_dir).unwrap();

    let err = shelf.restore_course(&entry.trash_id).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("biology"),
        "error must name the conflicting course id, got: {msg}"
    );
}

#[test]
fn empty_trash_permanently_removes_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::open_or_create(tmp.path()).unwrap();
    shelf.create_course("Biology", &[]).unwrap();

    let entry = shelf.trash_course("biology").unwrap();
    assert_eq!(shelf.list_trash().unwrap().len(), 1);

    shelf.empty_trash(&entry.trash_id).unwrap();
    assert_eq!(shelf.list_trash().unwrap().len(), 0);
    assert!(!tmp
        .path()
        .join(".tutordocs/trash")
        .join(&entry.trash_id)
        .exists());
}

#[test]
fn trash_and_restore_refuse_unknown_or_escaping_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::open_or_create(tmp.path()).unwrap();

    assert!(matches!(
        shelf.trash_course("missing"),
        Err(StoreError::CourseNotFound(_))
    ));
    assert!(matches!(
        shelf.trash_course("../escape"),
        Err(StoreError::InvalidCourseId(_))
    ));
    assert!(matches!(
        shelf.restore_course("missing"),
        Err(StoreError::Rejected(_))
    ));
    assert!(matches!(
        shelf.restore_course("../escape"),
        Err(StoreError::Rejected(_))
    ));
    assert!(matches!(
        shelf.empty_trash("missing"),
        Err(StoreError::Rejected(_))
    ));
    assert!(matches!(
        shelf.empty_trash("../escape"),
        Err(StoreError::Rejected(_))
    ));
}
