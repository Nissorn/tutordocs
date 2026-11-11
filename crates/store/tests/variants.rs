//! Tests for outline variants in crates/store.

use std::fs;
use tutordocs_store::{
    ChapterRef, Course, Library, Outline, OutlineChapter, OutlineStatus, StoreError,
};

#[test]
fn migration_sets_current_variant_for_course_with_outline_and_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("course");
    fs::create_dir_all(root.join("chapters/c1")).unwrap();
    fs::create_dir_all(root.join("sources")).unwrap();

    let legacy_course = Course {
        id: "test-course".to_string(),
        title: "Test Course".to_string(),
        objectives: vec!["Objective 1".to_string()],
        chapters: vec![ChapterRef {
            id: "c1".to_string(),
            title: "Chapter 1".to_string(),
            summary: "Summary 1".to_string(),
            sources: vec![],
            comment: "".to_string(),
        }],
        outline_status: OutlineStatus::Draft,
        current_variant_id: None,
        label: None,
        parent_id: None,
        created_at: None,
    };
    fs::write(
        root.join("course.json"),
        serde_json::to_string_pretty(&legacy_course).unwrap(),
    )
    .unwrap();

    // First open performs migration
    let lib = Library::open(&root).unwrap();
    let course1 = lib.course().unwrap();
    assert_eq!(course1.current_variant_id.as_deref(), Some("current"));
    assert_eq!(course1.label.as_deref(), Some("Current"));
    assert_eq!(course1.parent_id, None);
    let created_at1 = course1.created_at.clone();
    assert!(created_at1.is_some());
    // outlines/ directory has no file for current
    assert!(!root.join("outlines/current.json").exists());

    // Second open is idempotent and changes nothing
    let lib2 = Library::open(&root).unwrap();
    let course2 = lib2.course().unwrap();
    assert_eq!(course2.current_variant_id.as_deref(), Some("current"));
    assert_eq!(course2.created_at, created_at1);
}

#[test]
fn migration_leaves_no_outline_courses_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("course");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(root.join("sources")).unwrap();
    fs::create_dir_all(root.join("chapters")).unwrap();

    let no_outline = Course {
        id: "empty-course".to_string(),
        title: "Empty Course".to_string(),
        objectives: vec![],
        chapters: vec![],
        outline_status: OutlineStatus::Approved,
        current_variant_id: None,
        label: None,
        parent_id: None,
        created_at: None,
    };
    let initial_json = serde_json::to_string_pretty(&no_outline).unwrap();
    fs::write(root.join("course.json"), &initial_json).unwrap();

    let lib = Library::open(&root).unwrap();
    let course = lib.course().unwrap();
    assert_eq!(course.current_variant_id, None);
    assert_eq!(course.label, None);

    let current_json = fs::read_to_string(root.join("course.json")).unwrap();
    assert_eq!(current_json, initial_json);
}

#[test]
fn list_and_get_and_branch_and_swap_variants() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("course");
    let lib = Library::create(&root).unwrap();

    // Give it an outline
    let starter_outline = Outline {
        status: OutlineStatus::Draft,
        title: "My Course".to_string(),
        objectives: vec!["Learn basics".to_string()],
        chapters: vec![
            OutlineChapter {
                id: None,
                title: "Introduction".to_string(),
                summary: "Intro summary".to_string(),
                sources: vec![],
                comment: "".to_string(),
                written: false,
            },
            OutlineChapter {
                id: None,
                title: "Core Concepts".to_string(),
                summary: "Core summary".to_string(),
                sources: vec![],
                comment: "".to_string(),
                written: false,
            },
        ],
    };
    let saved = lib.save_outline(&starter_outline).unwrap();
    let ch1_id = saved.chapters[0].id.clone().unwrap();
    let ch2_id = saved.chapters[1].id.clone().unwrap();

    // Write chapter 1
    fs::write(
        root.join("chapters").join(&ch1_id).join("index.md"),
        "# Introduction\n\nWelcome.\n",
    )
    .unwrap();

    // Listing variants now shows "current"
    let variants = lib.list_outline_variants().unwrap();
    assert_eq!(variants.len(), 1);
    assert_eq!(variants[0].id, "current");
    assert_eq!(variants[0].label, "Current");
    assert_eq!(variants[0].chapter_count, 2);
    assert_eq!(variants[0].written_chapter_count, 1);

    // Get current variant
    let curr = lib.get_outline_variant("current").unwrap();
    assert_eq!(curr.id, "current");
    assert_eq!(curr.chapters.len(), 2);
    assert!(curr.chapters[0].written);
    assert!(!curr.chapters[1].written);

    // Branch to "Draft B"
    let draft_b = lib.branch_outline("current", "Draft B").unwrap();
    assert_eq!(draft_b.id, "draft-b");
    assert_eq!(draft_b.label, "Draft B");
    assert_eq!(draft_b.parent_id.as_deref(), Some("current"));
    assert_eq!(draft_b.chapters.len(), 2);
    assert!(draft_b.chapters[0].written);

    // outlines/draft-b.json exists, but current variant in course.json is still "current"
    assert!(root.join("outlines/draft-b.json").is_file());
    assert_eq!(
        lib.course().unwrap().current_variant_id.as_deref(),
        Some("current")
    );

    // Listing variants now shows 2
    let variants2 = lib.list_outline_variants().unwrap();
    assert_eq!(variants2.len(), 2);
    assert_eq!(variants2[0].id, "current");
    assert_eq!(variants2[1].id, "draft-b");

    // Saving draft-b with removal of written chapter is refused
    let invalid_draft_b = Outline {
        status: OutlineStatus::Draft,
        title: "My Course".to_string(),
        objectives: vec!["Learn basics".to_string()],
        chapters: vec![OutlineChapter {
            id: Some(ch2_id.clone()),
            title: "Core Concepts".to_string(),
            summary: "Core summary".to_string(),
            sources: vec![],
            comment: "".to_string(),
            written: false,
        }],
    };
    let err = lib
        .save_outline_variant("draft-b", &invalid_draft_b)
        .unwrap_err();
    assert!(matches!(err, StoreError::Rejected(_)));
    assert!(err.to_string().contains("cannot remove written chapter"));

    // Saving draft-b with rename and new order is allowed
    let valid_draft_b = Outline {
        status: OutlineStatus::Draft,
        title: "My Course (Variant)".to_string(),
        objectives: vec!["Learn basics".to_string(), "Advanced topic".to_string()],
        chapters: vec![
            OutlineChapter {
                id: Some(ch2_id.clone()),
                title: "Core Concepts (Moved First)".to_string(),
                summary: "Core summary".to_string(),
                sources: vec![],
                comment: "".to_string(),
                written: false,
            },
            OutlineChapter {
                id: Some(ch1_id.clone()),
                title: "Introduction (Moved Second)".to_string(),
                summary: "Intro summary".to_string(),
                sources: vec![],
                comment: "".to_string(),
                written: true,
            },
        ],
    };
    lib.save_outline_variant("draft-b", &valid_draft_b).unwrap();

    // Verify written chapter file was NOT modified by rename
    let content = fs::read_to_string(root.join("chapters").join(&ch1_id).join("index.md")).unwrap();
    assert_eq!(content, "# Introduction\n\nWelcome.\n");

    // Swap to draft-b
    let swapped = lib.set_current_variant("draft-b").unwrap();
    assert_eq!(swapped.id, "draft-b");
    assert_eq!(
        lib.course().unwrap().current_variant_id.as_deref(),
        Some("draft-b")
    );

    // Now draft-b is in course.json, outlines/draft-b.json is gone, and outlines/current.json exists
    assert!(!root.join("outlines/draft-b.json").exists());
    assert!(root.join("outlines/current.json").is_file());

    // get_outline() reads current variant (now draft-b)
    let active_outline = lib.outline().unwrap();
    assert_eq!(
        active_outline.chapters[0].title,
        "Core Concepts (Moved First)"
    );
    assert_eq!(
        active_outline.chapters[1].title,
        "Introduction (Moved Second)"
    );

    // After swapping to draft-b, listing variants has draft-b with current: true, and current with current: false
    let variants3 = lib.list_outline_variants().unwrap();
    assert_eq!(variants3.len(), 2);
    assert_eq!(variants3[0].id, "current");
    assert!(!variants3[0].current);
    assert_eq!(variants3[1].id, "draft-b");
    assert!(variants3[1].current);
}

#[test]
fn list_outline_variants_marks_current_even_when_not_oldest() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("course");
    let lib = Library::create(&root).unwrap();

    let starter = Outline {
        status: OutlineStatus::Draft,
        title: "Test Course".to_string(),
        objectives: vec![],
        chapters: vec![OutlineChapter {
            id: None,
            title: "Ch 1".to_string(),
            summary: "".to_string(),
            sources: vec![],
            comment: "".to_string(),
            written: false,
        }],
    };
    lib.save_outline(&starter).unwrap();

    let v1 = lib.list_outline_variants().unwrap();
    assert_eq!(v1.len(), 1);
    assert!(v1[0].current);
    assert_eq!(v1[0].id, "current");

    // Make the initial variant explicitly older
    let mut course = lib.course().unwrap();
    course.created_at = Some("2026-01-01T00:00:00Z".to_string());
    fs::write(
        root.join("course.json"),
        serde_json::to_string_pretty(&course).unwrap(),
    )
    .unwrap();

    let _ = lib.branch_outline("current", "Draft B").unwrap();
    lib.set_current_variant("draft-b").unwrap();

    let list = lib.list_outline_variants().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, "current");
    assert!(!list[0].current, "older variant is no longer current");
    assert_eq!(list[1].id, "draft-b");
    assert!(list[1].current, "newer variant Draft B is current");
}
