//! Public-seam tests for course setup and templates in `crates/store`.

use tutordocs_store::{list_course_templates, Course, CourseSetup, Library, StoreError};

fn temp_lib() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().expect("temp dir");
    let lib = Library::create(dir.path()).expect("create library");
    (dir, lib)
}

#[test]
fn templates_list_in_contract_order_with_titles_and_descriptions() {
    let templates = list_course_templates();
    let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["interview", "exam", "overview", "custom"]);
    assert_eq!(templates[0].title, "Interview prep");
    assert_eq!(templates[1].title, "Exam review");
    assert_eq!(templates[2].title, "Quick overview");
    for t in &templates {
        assert!(!t.title.trim().is_empty(), "{} missing title", t.id);
        assert!(
            !t.description.trim().is_empty(),
            "{} missing description",
            t.id
        );
    }
}

#[test]
fn a_course_starts_with_no_setup_and_persists_valid_setup_in_course_json() {
    let (dir, lib) = temp_lib();
    assert_eq!(lib.course_setup().unwrap(), None);

    let setup = CourseSetup {
        template: "interview".to_string(),
        level: "some".to_string(),
        hours_per_week: Some(6),
        language: "th".to_string(),
        guide: "Focus on distributed consensus.".to_string(),
    };
    let saved = lib.set_course_setup(&setup).unwrap();
    assert_eq!(saved, setup);

    let reopened = Library::open(dir.path()).unwrap();
    assert_eq!(reopened.course_setup().unwrap(), Some(setup.clone()));

    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("course.json")).unwrap())
            .unwrap();
    assert_eq!(
        raw["setup"],
        serde_json::json!({
            "template": "interview",
            "level": "some",
            "hoursPerWeek": 6,
            "language": "th",
            "guide": "Focus on distributed consensus."
        })
    );

    // Writing a new outline keeps the saved setup in course.json.
    reopened
        .write_course(&Course {
            id: "course".to_string(),
            title: "Consensus".to_string(),
            objectives: vec!["Explain Raft".to_string()],
            chapters: Vec::new(),
            ..Course::default()
        })
        .unwrap();
    assert_eq!(reopened.course_setup().unwrap(), Some(setup));
}

#[test]
fn set_course_setup_rejects_unknown_template_level_language_or_zero_hours() {
    let (dir, lib) = temp_lib();
    let valid = CourseSetup {
        template: "exam".to_string(),
        level: "new".to_string(),
        hours_per_week: None,
        language: "en".to_string(),
        guide: String::new(),
    };

    let bad_template = CourseSetup {
        template: "unknown".to_string(),
        ..valid.clone()
    };
    assert!(matches!(
        lib.set_course_setup(&bad_template),
        Err(StoreError::Rejected(_))
    ));

    let bad_level = CourseSetup {
        level: "expert".to_string(),
        ..valid.clone()
    };
    assert!(matches!(
        lib.set_course_setup(&bad_level),
        Err(StoreError::Rejected(_))
    ));

    let bad_lang = CourseSetup {
        language: "fr".to_string(),
        ..valid.clone()
    };
    assert!(matches!(
        lib.set_course_setup(&bad_lang),
        Err(StoreError::Rejected(_))
    ));

    let bad_hours = CourseSetup {
        hours_per_week: Some(0),
        ..valid
    };
    assert!(matches!(
        lib.set_course_setup(&bad_hours),
        Err(StoreError::Rejected(_))
    ));

    // Nothing was written on rejection.
    assert_eq!(
        Library::open(dir.path()).unwrap().course_setup().unwrap(),
        None
    );
}
