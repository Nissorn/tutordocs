//! Public-seam tests for `import_files` and `list_course_sources` (`Library::course_sources`).

use std::fs;
use std::path::PathBuf;

use tutordocs_store::{ImportStatus, Library, SourceKind, SourceOrigin};

fn temp_lib() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().expect("temp dir");
    let lib = Library::create(dir.path().join("course")).expect("create library");
    (dir, lib)
}

#[test]
fn imports_pdf_md_and_txt_skips_unsupported_types_and_leaves_originals_untouched() {
    let (dir, lib) = temp_lib();
    let incoming = dir.path().join("incoming");
    fs::create_dir_all(&incoming).unwrap();

    let pdf_path = incoming.join("slides.pdf");
    let md_path = incoming.join("notes.md");
    let markdown_path = incoming.join("extra.markdown");
    let txt_path = incoming.join("handout.txt");
    let png_path = incoming.join("diagram.png");

    fs::write(&pdf_path, b"%PDF-1.7 sample").unwrap();
    fs::write(&md_path, "# Notes\n").unwrap();
    fs::write(&markdown_path, "# Extra\n").unwrap();
    fs::write(&txt_path, "Plain text handout\n").unwrap();
    fs::write(&png_path, b"\x89PNG").unwrap();

    let results = lib
        .import_files(&[&pdf_path, &md_path, &markdown_path, &txt_path, &png_path])
        .unwrap();

    assert_eq!(results.len(), 5);
    assert_eq!(results[0].status, ImportStatus::Imported);
    assert_eq!(results[0].reason, None);
    assert_eq!(results[0].source.as_ref().unwrap().kind, SourceKind::Pdf);
    assert_eq!(results[0].source.as_ref().unwrap().bytes, 15);

    assert_eq!(results[1].status, ImportStatus::Imported);
    assert_eq!(
        results[1].source.as_ref().unwrap().kind,
        SourceKind::Markdown
    );

    assert_eq!(results[2].status, ImportStatus::Imported);
    assert_eq!(
        results[2].source.as_ref().unwrap().kind,
        SourceKind::Markdown
    );

    assert_eq!(results[3].status, ImportStatus::Imported);
    assert_eq!(results[3].source.as_ref().unwrap().kind, SourceKind::Text);

    assert_eq!(results[4].status, ImportStatus::Skipped);
    assert!(results[4]
        .reason
        .as_deref()
        .unwrap()
        .contains("unsupported"));
    assert_eq!(results[4].source, None);

    // Originals are untouched at their original paths.
    assert_eq!(fs::read(&pdf_path).unwrap(), b"%PDF-1.7 sample");
    assert_eq!(fs::read_to_string(&md_path).unwrap(), "# Notes\n");
    assert_eq!(
        fs::read_to_string(&txt_path).unwrap(),
        "Plain text handout\n"
    );

    // Also verify URL sources list with kind "url" and origin metadata.
    let staged = lib.staging_dir().unwrap().join("captured.md");
    fs::write(&staged, "# Web\n").unwrap();
    lib.commit_capture(
        &staged,
        "web.md",
        &SourceOrigin {
            url: "https://example.com/a".to_string(),
            fetched_at: "2026-11-09T00:00:00Z".to_string(),
        },
    )
    .unwrap();

    let listed = lib.course_sources().unwrap();
    let summary: Vec<(&str, SourceKind, Option<&str>)> = listed
        .iter()
        .map(|s| (s.name.as_str(), s.kind, s.url.as_deref()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("extra.markdown", SourceKind::Markdown, None),
            ("handout.txt", SourceKind::Text, None),
            ("notes.md", SourceKind::Markdown, None),
            ("slides.pdf", SourceKind::Pdf, None),
            ("web.md", SourceKind::Url, Some("https://example.com/a")),
        ]
    );
}

#[test]
fn skips_duplicate_bytes_and_makes_name_clashes_unique() {
    let (dir, lib) = temp_lib();
    let folder_a = dir.path().join("a");
    let folder_b = dir.path().join("b");
    fs::create_dir_all(&folder_a).unwrap();
    fs::create_dir_all(&folder_b).unwrap();

    let first = folder_a.join("notes.md");
    let duplicate_diff_name = folder_b.join("copy.md");
    let clash_diff_bytes = folder_b.join("notes.md");

    fs::write(&first, "same content\n").unwrap();
    fs::write(&duplicate_diff_name, "same content\n").unwrap();
    fs::write(&clash_diff_bytes, "different content\n").unwrap();

    let results = lib
        .import_files(&[&first, &duplicate_diff_name, &clash_diff_bytes])
        .unwrap();

    assert_eq!(results[0].status, ImportStatus::Imported);
    assert_eq!(results[0].source.as_ref().unwrap().name, "notes.md");

    assert_eq!(results[1].status, ImportStatus::Skipped);
    assert_eq!(results[1].reason.as_deref(), Some("duplicate"));
    assert_eq!(results[1].source, None);

    assert_eq!(results[2].status, ImportStatus::Imported);
    assert_eq!(results[2].source.as_ref().unwrap().name, "notes-2.md");

    // Re-importing the first file again is also skipped as "duplicate".
    let again = lib.import_files(&[&first]).unwrap();
    assert_eq!(again[0].status, ImportStatus::Skipped);
    assert_eq!(again[0].reason.as_deref(), Some("duplicate"));

    let names: Vec<String> = lib
        .course_sources()
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, vec!["notes.md", "notes-2.md"]);
}

#[test]
fn recurses_a_folder_to_depth_3_and_ignores_deeper_levels() {
    let (dir, lib) = temp_lib();
    let root_folder = dir.path().join("pack");
    let d1 = root_folder.join("l1");
    let d2 = d1.join("l2");
    let d3 = d2.join("l3");
    let d4 = d3.join("l4");
    fs::create_dir_all(&d4).unwrap();

    fs::write(root_folder.join("d0.md"), "level 0").unwrap();
    fs::write(d1.join("d1.md"), "level 1").unwrap();
    fs::write(d2.join("d2.md"), "level 2").unwrap();
    fs::write(d3.join("d3.md"), "level 3").unwrap();
    fs::write(d4.join("d4.md"), "level 4 too deep").unwrap();

    let results = lib.import_files(&[&root_folder]).unwrap();
    let imported: Vec<String> = results
        .into_iter()
        .filter_map(|r| r.source.map(|s| s.name))
        .collect();
    assert_eq!(imported, vec!["d0.md", "d1.md", "d2.md", "d3.md"]);
}

#[test]
fn caps_folder_import_at_200_files() {
    let (dir, lib) = temp_lib();
    let big = dir.path().join("big");
    fs::create_dir_all(&big).unwrap();

    for i in 0..235 {
        fs::write(big.join(format!("file-{i:03}.txt")), format!("content {i}")).unwrap();
    }

    let results = lib.import_files(&[&big]).unwrap();
    assert_eq!(results.len(), 200);
    assert_eq!(lib.course_sources().unwrap().len(), 200);
}

#[test]
fn does_not_follow_symlinks_out_of_the_chosen_folder() {
    let (dir, lib) = temp_lib();
    let outside = dir.path().join("outside");
    let chosen = dir.path().join("chosen");
    fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(&chosen).unwrap();

    let secret_file = outside.join("secret.md");
    fs::write(&secret_file, "secret outside file").unwrap();
    let outside_dir = outside.join("secret_dir");
    fs::create_dir_all(&outside_dir).unwrap();
    fs::write(
        outside_dir.join("nested_secret.md"),
        "secret in outside dir",
    )
    .unwrap();

    fs::write(chosen.join("inside.md"), "safe inside file").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&secret_file, chosen.join("escape_file.md")).unwrap();
        std::os::unix::fs::symlink(&outside_dir, chosen.join("escape_dir")).unwrap();
    }

    let results = lib.import_files(&[PathBuf::from(&chosen)]).unwrap();
    let sources = lib.course_sources().unwrap();
    let names: Vec<&str> = sources.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["inside.md"]);
    assert!(results.iter().any(|r| r.status == ImportStatus::Skipped));
}
