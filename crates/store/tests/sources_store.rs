//! Public-seam tests for the sources API: what chapter generation and the UI
//! see when they list and read `sources/`, whether a file was dropped in by
//! the user or captured from a URL. Temp directories only.

use tutordocs_store::{Library, SourceOrigin, StoreError};

fn temp_lib() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().expect("temp dir");
    let lib = Library::create(dir.path()).expect("create library");
    (dir, lib)
}

#[test]
fn an_imported_markdown_file_is_listed_and_readable() {
    let (dir, lib) = temp_lib();
    let sources = dir.path().join("sources");
    std::fs::write(sources.join("notes.md"), "# Notes\n\nBody.\n").unwrap();
    std::fs::write(sources.join("lecture.pdf"), b"%PDF-1.7").unwrap();
    std::fs::write(sources.join(".hidden.md"), "not a source").unwrap();

    let listed = lib.sources().unwrap();
    let names: Vec<&str> = listed.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["notes.md"]);
    assert_eq!(listed[0].path, sources.join("notes.md"));
    assert_eq!(listed[0].origin, None);
    assert_eq!(lib.read_source("notes.md").unwrap(), "# Notes\n\nBody.\n");
}

fn origin(url: &str) -> SourceOrigin {
    SourceOrigin {
        url: url.to_string(),
        fetched_at: "2023-11-14T22:13:20Z".to_string(),
    }
}

fn stage(lib: &Library, file: &str, markdown: &str) -> std::path::PathBuf {
    let staged = lib.staging_dir().unwrap().join(file);
    std::fs::write(&staged, markdown).unwrap();
    staged
}

#[test]
fn a_committed_capture_lists_with_its_origin_and_reads_like_any_source() {
    let (dir, lib) = temp_lib();
    let staged = stage(&lib, "job-1.md", "# Article\n\nText.\n");
    let from = origin("https://example.com/a");

    let committed = lib
        .commit_capture(&staged, "example-com-a.md", &from)
        .unwrap();

    assert_eq!(committed.name, "example-com-a.md");
    assert_eq!(committed.origin, Some(from.clone()));
    assert!(!staged.exists(), "the staged file moves, it is not copied");
    drop(lib);

    let lib = Library::open(dir.path()).unwrap();
    let listed = lib.sources().unwrap();
    assert_eq!(listed, vec![committed]);
    assert_eq!(listed[0].origin, Some(from));
    assert_eq!(
        lib.read_source("example-com-a.md").unwrap(),
        "# Article\n\nText.\n"
    );
}

#[test]
fn committing_over_an_existing_source_is_refused_and_changes_nothing() {
    let (_dir, lib) = temp_lib();
    let first = stage(&lib, "one.md", "first\n");
    lib.commit_capture(&first, "page.md", &origin("https://example.com/one"))
        .unwrap();
    let second = stage(&lib, "two.md", "second\n");

    let err = lib
        .commit_capture(&second, "page.md", &origin("https://example.com/two"))
        .unwrap_err();

    assert!(matches!(err, StoreError::SourceExists(_)), "got: {err}");
    assert_eq!(lib.read_source("page.md").unwrap(), "first\n");
    assert_eq!(
        lib.sources().unwrap()[0].origin,
        Some(origin("https://example.com/one"))
    );
}

#[test]
fn a_source_name_cannot_reach_outside_sources() {
    let (dir, lib) = temp_lib();
    std::fs::write(dir.path().join("secret.md"), "outside").unwrap();

    for bad in [
        "../secret.md",
        "sub/x.md",
        "/etc/passwd.md",
        "",
        "..",
        ".hidden.md",
        "notes.txt",
    ] {
        assert!(
            matches!(lib.read_source(bad), Err(StoreError::InvalidSourceName(_))),
            "read {bad:?}"
        );
        let staged = stage(&lib, "s.md", "x");
        assert!(
            matches!(
                lib.commit_capture(&staged, bad, &origin("https://example.com")),
                Err(StoreError::InvalidSourceName(_))
            ),
            "commit {bad:?}"
        );
    }

    assert!(lib.sources().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("secret.md")).unwrap(),
        "outside"
    );
    assert!(!dir.path().join("secret.md.source.json").exists());
    assert!(matches!(
        lib.read_source("missing.md"),
        Err(StoreError::SourceNotFound(_))
    ));
}

#[test]
fn the_origin_is_recorded_beside_the_file_as_plain_json() {
    let (dir, lib) = temp_lib();
    let staged = stage(&lib, "job.md", "body\n");
    lib.commit_capture(&staged, "page.md", &origin("https://example.com/p?x=1"))
        .unwrap();

    let beside = dir.path().join("sources/page.md.source.json");
    let recorded: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(beside).unwrap()).unwrap();
    assert_eq!(
        recorded,
        serde_json::json!({
            "url": "https://example.com/p?x=1",
            "fetched_at": "2023-11-14T22:13:20Z",
        })
    );
}

#[test]
fn an_unreadable_origin_file_does_not_hide_the_source() {
    let (dir, lib) = temp_lib();
    let sources = dir.path().join("sources");
    std::fs::write(sources.join("page.md"), "body\n").unwrap();
    std::fs::write(sources.join("page.md.source.json"), "{ not json").unwrap();

    let listed = lib.sources().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "page.md");
    assert_eq!(listed[0].origin, None);
}

#[test]
fn sources_list_by_name_so_a_numbered_copy_follows_its_original() {
    let (dir, lib) = temp_lib();
    let sources = dir.path().join("sources");
    for name in ["b.md", "a-2.md", "a.md"] {
        std::fs::write(sources.join(name), "x").unwrap();
    }

    let names: Vec<String> = lib.sources().unwrap().into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["a.md", "a-2.md", "b.md"]);
}
