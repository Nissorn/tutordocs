//! Tests for course renaming.

use tutordocs_store::{Library, Shelf, StoreError};

#[test]
fn rename_changes_title_and_preserves_folder_id() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::open_or_create(tmp.path()).unwrap();
    let initial = shelf.create_course("Cell Biology", &[]).unwrap();
    let course_id = initial.id;
    assert_eq!(course_id, "cell-biology");

    let updated = shelf
        .rename_course(&course_id, "Advanced Cell Biology")
        .unwrap();
    assert_eq!(updated.id, "cell-biology");
    assert_eq!(updated.title, "Advanced Cell Biology");

    // The shelf folder is unchanged
    let dir = shelf.course_dir(&course_id).unwrap();
    assert_eq!(dir, tmp.path().join("courses/cell-biology"));

    // Reading the course directly verifies the file was updated
    let lib = Library::open(&dir).unwrap();
    let course = lib.course().unwrap();
    assert_eq!(course.title, "Advanced Cell Biology");
    assert_eq!(course.id, initial.stored_id);
}

#[test]
fn rename_refuses_empty_title() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::open_or_create(tmp.path()).unwrap();
    let initial = shelf.create_course("Cell Biology", &[]).unwrap();

    let err1 = shelf.rename_course(&initial.id, "   ").unwrap_err();
    assert!(matches!(err1, StoreError::Rejected(_)));

    let err2 = shelf.rename_course(&initial.id, "").unwrap_err();
    assert!(matches!(err2, StoreError::Rejected(_)));
}

#[test]
fn rename_refuses_unknown_or_escaping_course_id() {
    let tmp = tempfile::tempdir().unwrap();
    let shelf = Shelf::open_or_create(tmp.path()).unwrap();

    assert!(matches!(
        shelf.rename_course("missing", "New Title"),
        Err(StoreError::CourseNotFound(_))
    ));
    assert!(matches!(
        shelf.rename_course("../escape", "New Title"),
        Err(StoreError::InvalidCourseId(_))
    ));
}
