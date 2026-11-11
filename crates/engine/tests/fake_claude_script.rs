//! The acceptance run's fake `claude` script, driven by the real `ClaudeDriver`
//! and the real engine. A bad script would cost a whole desktop run to find,
//! so this proves the outline, the first chapter and the rewrite here first.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use tutordocs_agent::ClaudeDriver;
use tutordocs_engine::{generate_outline, ApproveGenerate, ChapterJobs};
use tutordocs_store::Library;

fn repo(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

#[tokio::test]
async fn the_script_gives_the_recorded_outline_chapter_and_rewrite() {
    let scratch = tempfile::tempdir().unwrap();
    let state = scratch.path().join("state");
    fs::create_dir(&state).unwrap();
    let fake = repo("crates/engine/tests/fixtures/fake-claude");
    let wrapper = scratch.path().join("claude");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nexport FAKE_DIR={dir}\nexport FAKE_CHAPTERS={chapters}\nexport FAKE_STATE={state}\nexec {dir}/claude \"$@\"\n",
            dir = fake.canonicalize().unwrap().display(),
            chapters = repo("ui/src/fixtures/feel").canonicalize().unwrap().display(),
            state = state.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let library = scratch.path().join("library");
    Library::create(&library).unwrap();

    let driver = ClaudeDriver::new(&wrapper).unwrap();
    let course = generate_outline(&library, &driver, "Rust ownership", &[])
        .await
        .unwrap();
    let ids: Vec<&str> = course.chapters.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["moves", "borrows", "lifetimes"]);

    let jobs = ChapterJobs::new();
    jobs.approve(&library, ApproveGenerate::OnOpen).unwrap();
    let first = jobs
        .open(&library, &driver, "moves", &|_| {})
        .await
        .unwrap();
    assert_eq!(
        first,
        fs::read_to_string(repo("ui/src/fixtures/feel/chapter-v1.md")).unwrap()
    );
    let second = jobs
        .regenerate(&library, &driver, "moves", None, &|_| {})
        .await
        .unwrap();
    assert_eq!(
        second,
        fs::read_to_string(repo("ui/src/fixtures/feel/chapter-v2.md")).unwrap()
    );

    let log = fs::read_to_string(state.join("calls.log")).unwrap();
    assert_eq!(
        log.lines().count(),
        3,
        "one outline job and two chapter jobs: {log}"
    );
    assert!(
        log.lines().all(|l| l.ends_with("anthropic_key=unset")),
        "{log}"
    );
}
