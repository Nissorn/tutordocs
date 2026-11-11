//! Outline generation through the agent, against a fake `AgentDriver`. The
//! assertions read the library back through the store's public API.

use tutordocs_engine::{generate_outline, revise_outline, GenerationError};
use tutordocs_store::{ChapterRef, Course, Library, OutlineStatus};

mod course_support;
use course_support::{prompts, ran, setup, Fake};

#[tokio::test]
async fn a_topic_becomes_a_course_json_with_every_chapter_a_placeholder() {
    let env = setup(Fake::default());

    let course = generate_outline(&env.library, &env.driver, "Rust ownership", &[])
        .await
        .unwrap();

    let lib = Library::open(&env.library).unwrap();
    assert_eq!(lib.course().unwrap(), course);
    assert_eq!(course.outline_status, OutlineStatus::Draft);
    assert_eq!(lib.outline().unwrap().status, OutlineStatus::Draft);
    assert_eq!(course.id, "library", "the library keeps its course id");
    assert_eq!(course.title, "Rust ownership");
    assert_eq!(
        course.objectives,
        vec![
            "Explain what a move does",
            "Choose between shared and unique borrows"
        ]
    );
    let chapters = lib.chapters().unwrap();
    let ids: Vec<&str> = chapters.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec!["moves", "borrows", "lifetimes"]);
    assert!(chapters.iter().all(|c| c.is_placeholder && c.path.is_dir()));
    assert_eq!(
        course.chapters[1].summary,
        "Shared and unique references and the rules that keep them safe"
    );
}

#[tokio::test]
async fn the_outline_job_is_read_only_in_a_staging_folder_holding_only_the_chosen_sources() {
    let env = setup(Fake::default());
    std::fs::write(env.library.join("sources/lecture-3.md"), "# Ownership\n").unwrap();
    std::fs::write(env.library.join("sources/other.md"), "# Other\n").unwrap();
    std::fs::write(env.library.join("notes.txt"), "not a source\n").unwrap();

    generate_outline(
        &env.library,
        &env.driver,
        "Rust ownership",
        &["lecture-3.md".to_string()],
    )
    .await
    .unwrap();

    let ran = ran(&env);
    assert_eq!(ran.len(), 1, "{ran:?}");
    let job = &ran[0].job;
    assert!(
        job.timeout.is_none(),
        "an outline job has no wall-clock limit"
    );
    assert!(job.read_only, "the outline job plans and writes nothing");
    assert!(
        job.json_schema.is_some(),
        "the outline is structured output"
    );
    assert_eq!(
        job.allowed_tools,
        Some(vec![
            "Read".to_string(),
            "Grep".to_string(),
            "Glob".to_string(),
        ])
    );
    let staging = Library::open(&env.library).unwrap().staging_dir().unwrap();
    assert_eq!(job.workdir.parent(), Some(staging.as_path()));
    assert_ne!(job.workdir, env.library);
    assert_eq!(
        ran[0].files_at_start,
        vec!["sources/".to_string(), "sources/lecture-3.md".to_string(),]
    );
    assert_eq!(
        ran[0].texts.get("sources/lecture-3.md").map(String::as_str),
        Some("# Ownership\n")
    );
    assert!(
        !job.workdir.exists(),
        "the staging folder is removed afterwards"
    );
    assert!(
        course_support::names_in(&staging).is_empty(),
        "staging holds nothing after the job"
    );
    assert_eq!(
        std::fs::read_to_string(env.library.join("sources/lecture-3.md")).unwrap(),
        "# Ownership\n"
    );
    assert_eq!(
        std::fs::read_to_string(env.library.join("sources/other.md")).unwrap(),
        "# Other\n"
    );
    let library_path = env.library.to_str().unwrap();
    assert!(
        !job.prompt.contains(library_path),
        "the prompt must not hand the agent the library path"
    );
}

#[tokio::test]
async fn the_outline_prompt_names_the_topic_and_the_chosen_sources_and_says_to_search_the_folder() {
    let env = setup(Fake::default());
    std::fs::write(env.library.join("sources/lecture-3.md"), "# Ownership\n").unwrap();

    generate_outline(
        &env.library,
        &env.driver,
        "Rust ownership",
        &["lecture-3.md".to_string()],
    )
    .await
    .unwrap();

    let prompt = &prompts(&env)[0];
    assert!(prompt.contains("Rust ownership"), "{prompt}");
    assert!(prompt.contains("sources/lecture-3.md"), "{prompt}");
    assert!(prompt.contains("search"), "{prompt}");
}

#[tokio::test]
async fn a_source_that_is_not_in_the_library_is_refused_before_the_agent_runs() {
    let env = setup(Fake::default());

    let err = generate_outline(
        &env.library,
        &env.driver,
        "Rust ownership",
        &["missing.md".to_string()],
    )
    .await
    .unwrap_err();

    assert!(matches!(err, GenerationError::Store(_)), "{err:?}");
    assert!(ran(&env).is_empty());
}

#[tokio::test]
async fn an_empty_topic_with_no_sources_never_reaches_the_agent() {
    let env = setup(Fake::default());

    let err = generate_outline(&env.library, &env.driver, "   ", &[])
        .await
        .unwrap_err();

    assert!(matches!(err, GenerationError::InvalidRequest(_)), "{err:?}");
    assert!(ran(&env).is_empty());
}

async fn assert_outline_rejected(fixture: &str, why: &str) {
    let env = setup(Fake {
        outline: fixture,
        ..Fake::default()
    });
    let before = std::fs::read_to_string(env.library.join("course.json")).unwrap();

    let err = generate_outline(&env.library, &env.driver, "Rust ownership", &[])
        .await
        .unwrap_err();

    let reason = err.to_string();
    assert!(reason.contains(why), "reason was: {reason}");
    let after = std::fs::read_to_string(env.library.join("course.json")).unwrap();
    assert_eq!(after, before, "course.json is untouched");
    assert!(course_support::names_in(&env.library.join("chapters")).is_empty());
}

#[tokio::test]
async fn an_outline_with_duplicate_chapter_ids_is_rejected_and_nothing_is_written() {
    assert_outline_rejected("outline-duplicate-ids.json", "moves").await;
}

#[tokio::test]
async fn an_outline_whose_chapter_id_is_not_a_slug_is_rejected_and_nothing_is_written() {
    assert_outline_rejected("outline-bad-id.json", "../escape").await;
}

#[tokio::test]
async fn generate_outline_writes_a_draft_and_puts_setup_and_its_template_into_the_prompt() {
    let env = setup(Fake::default());
    let with_setup = serde_json::json!({
        "id": "library",
        "title": "Rust ownership",
        "objectives": [],
        "chapters": [],
        "setup": {
            "template": "interview",
            "level": "some",
            "hoursPerWeek": 6,
            "language": "th",
            "guide": "Focus on live-coding ownership puzzles"
        }
    });
    std::fs::write(
        env.library.join("course.json"),
        serde_json::to_string_pretty(&with_setup).unwrap(),
    )
    .unwrap();

    let course = generate_outline(&env.library, &env.driver, "Rust ownership", &[])
        .await
        .unwrap();

    assert_eq!(course.outline_status, OutlineStatus::Draft);
    let prompt = &prompts(&env)[0];
    let expected_tpl = tutordocs_store::template_instructions_for_id("interview").unwrap();
    assert!(prompt.contains("TEMPLATE: interview"), "{prompt}");
    assert!(prompt.contains(expected_tpl), "{prompt}");
    assert!(prompt.contains("some"), "{prompt}");
    assert!(prompt.contains("6"), "{prompt}");
    assert!(prompt.contains("th"), "{prompt}");
    assert!(
        prompt.contains("Focus on live-coding ownership puzzles"),
        "{prompt}"
    );

    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(env.library.join("course.json")).unwrap())
            .unwrap();
    assert_eq!(disk["outlineStatus"], "draft");
    assert_eq!(disk["setup"]["template"], "interview");
    assert_eq!(
        disk["setup"]["guide"],
        "Focus on live-coding ownership puzzles"
    );
}

#[tokio::test]
async fn revise_outline_sends_comments_and_objectives_keeps_written_chapters_and_clears_comments() {
    let env = setup(Fake::default());
    let lib = Library::open(&env.library).unwrap();
    lib.write_course(&Course {
        id: "library".to_string(),
        title: "Rust ownership".to_string(),
        objectives: vec!["Master ownership".to_string()],
        chapters: vec![
            ChapterRef {
                id: "intro".to_string(),
                title: "Introduction".to_string(),
                summary: "Why ownership exists".to_string(),
                sources: vec![],
                comment: "Keep the stack vs heap intro".to_string(),
            },
            ChapterRef {
                id: "borrows".to_string(),
                title: "Borrows".to_string(),
                summary: "References".to_string(),
                sources: vec![],
                comment: "Expand on mutable aliasing".to_string(),
            },
        ],
        outline_status: OutlineStatus::Draft,
        ..Course::default()
    })
    .unwrap();
    std::fs::write(env.library.join("chapters/intro/index.md"), "# Intro\n").unwrap();

    let revised = revise_outline(&env.library, &env.driver, None)
        .await
        .unwrap();

    let prompt = &prompts(&env)[0];
    assert!(prompt.contains("Master ownership"), "{prompt}");
    assert!(prompt.contains("Keep the stack vs heap intro"), "{prompt}");
    assert!(prompt.contains("Expand on mutable aliasing"), "{prompt}");

    assert_eq!(revised.status, OutlineStatus::Draft);
    let intro = revised
        .chapters
        .iter()
        .find(|c| c.id.as_deref() == Some("intro"))
        .expect("written chapter intro is kept with its id");
    assert!(intro.written);
    assert!(
        revised.chapters.iter().all(|c| c.comment.is_empty()),
        "every comment is cleared: {revised:?}"
    );
    assert_eq!(
        Library::open(&env.library).unwrap().outline().unwrap(),
        revised
    );
}

#[tokio::test]
async fn revise_outline_shows_chapter_sources_in_prompt_and_keeps_sources_when_agent_omits_them() {
    let env = setup(Fake::default());
    let lib = Library::open(&env.library).unwrap();
    lib.write_course(&Course {
        id: "library".to_string(),
        title: "Rust ownership".to_string(),
        objectives: vec!["Master ownership".to_string()],
        chapters: vec![
            ChapterRef {
                id: "moves".to_string(),
                title: "Moves".to_string(),
                summary: "Ownership transfer".to_string(),
                sources: vec!["moves-slides.pdf".to_string()],
                comment: "Keep the stack example".to_string(),
            },
            ChapterRef {
                id: "borrows".to_string(),
                title: "Borrows".to_string(),
                summary: "References".to_string(),
                sources: vec!["borrows-notes.md".to_string(), "glossary.txt".to_string()],
                comment: "Expand mutable borrows".to_string(),
            },
        ],
        outline_status: OutlineStatus::Draft,
        ..Course::default()
    })
    .unwrap();
    std::fs::write(env.library.join("chapters/moves/index.md"), "# Moves\n").unwrap();

    let revised = revise_outline(&env.library, &env.driver, None)
        .await
        .unwrap();

    let prompt = &prompts(&env)[0];
    assert!(
        prompt.contains("moves-slides.pdf"),
        "revise prompt must show written chapter's current sources: {prompt}"
    );
    assert!(
        prompt.contains("borrows-notes.md") && prompt.contains("glossary.txt"),
        "revise prompt must show existing chapter's current sources: {prompt}"
    );

    let moves = revised
        .chapters
        .iter()
        .find(|c| c.id.as_deref() == Some("moves"))
        .expect("moves chapter present");
    assert_eq!(moves.sources, vec!["moves-slides.pdf".to_string()]);

    let borrows = revised
        .chapters
        .iter()
        .find(|c| c.id.as_deref() == Some("borrows"))
        .expect("borrows chapter present");
    assert_eq!(
        borrows.sources,
        vec!["borrows-notes.md".to_string(), "glossary.txt".to_string()]
    );
}
