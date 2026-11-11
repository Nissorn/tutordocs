//! Subprocess tests for `tutordocs-cli`. Every test runs the real binary
//! against a temp library, with a fake `claude` first on PATH and no API key
//! in the environment. Nothing here touches `~/TutorDocs`: HOME is a temp
//! folder and the library comes from `TUTORDOCS_LIBRARY` or `--library`.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tutordocs_engine::{ChapterContext, ChapterQueue, OutlineSummary};
use tutordocs_store::{
    ChapterAnswer, ChapterDoc, ChapterSummary, CourseSetup, CourseSource, CourseSummary,
    ImportResult, Outline, OutlineVariant, Template, TrashEntry, VariantMeta,
};

const CLI: &str = env!("CARGO_BIN_EXE_tutordocs-cli");
const FAKE_CLAUDE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake-claude");

/// What one run of the binary produced.
struct Out {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Out {
    fn context(&self) -> String {
        format!(
            "code {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.code, self.stdout, self.stderr
        )
    }

    /// Assert success and read stdout as the contract type `T`.
    fn ok<T: DeserializeOwned>(&self) -> T {
        assert_eq!(self.code, Some(0), "{}", self.context());
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| {
            panic!("stdout is not the expected shape ({e})\n{}", self.context())
        })
    }

    fn value(&self) -> Value {
        self.ok()
    }

    /// Assert exit 1 with `{"error": "..."}` alone on stdout; return the message.
    fn refused(&self) -> String {
        assert_eq!(self.code, Some(1), "{}", self.context());
        let value: Value = serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e})\n{}", self.context()));
        let object = value.as_object().expect("an error is a JSON object");
        assert_eq!(object.len(), 1, "{}", self.context());
        object["error"]
            .as_str()
            .expect("error is a string")
            .to_string()
    }

    /// Assert exit 2 and nothing on stdout.
    fn usage_error(&self) {
        assert_eq!(self.code, Some(2), "{}", self.context());
        assert_eq!(self.stdout, "", "{}", self.context());
    }

    /// Every stderr line that is a streamed job line, in order.
    fn job_lines(&self) -> Vec<Value> {
        self.stderr
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect()
    }
}

/// A temp library, a fake `claude` and some files to import.
struct Env {
    tmp: tempfile::TempDir,
    library: PathBuf,
    home: PathBuf,
    state: PathBuf,
    inputs: PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let bin = tmp.path().join("bin");
        let inputs = tmp.path().join("inputs");
        for dir in [&home, &bin, &inputs] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::copy(FAKE_CLAUDE, bin.join("claude")).unwrap();
        std::fs::write(
            inputs.join("study-notes.md"),
            "# Study notes\n\nA membrane is a lipid bilayer.\n",
        )
        .unwrap();
        std::fs::write(
            inputs.join("glossary.txt"),
            "bilayer: two layers of lipid\n",
        )
        .unwrap();
        std::fs::write(inputs.join("slides.docx"), "not a supported type").unwrap();
        Self {
            library: tmp.path().join("library"),
            state: tmp.path().join("fake-state"),
            tmp,
            home,
            inputs,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let path = format!("{}:/usr/bin:/bin", self.tmp.path().join("bin").display());
        let mut command = Command::new(CLI);
        command
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", path)
            .env("TUTORDOCS_LIBRARY", &self.library)
            .env("FAKE_STATE", &self.state)
            .env("FAKE_PAUSE", "0")
            .args(args);
        command
    }

    fn run(&self, args: &[&str]) -> Out {
        let output = self.command(args).output().expect("the binary runs");
        Out {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn input(&self, name: &str) -> String {
        self.inputs.join(name).display().to_string()
    }

    fn course_dir(&self, course: &str) -> PathBuf {
        self.library.join("courses").join(course)
    }

    /// A course with two imported sources.
    fn course(&self) -> String {
        let created: CourseSummary = self.run(&["course", "create", "Cell Membranes"]).ok();
        let notes = self.input("study-notes.md");
        let glossary = self.input("glossary.txt");
        self.run(&["import", "--course", &created.id, &notes, &glossary])
            .ok::<Vec<ImportResult>>();
        created.id
    }

    /// A course with a draft outline of four chapters.
    fn drafted(&self) -> String {
        let id = self.course();
        self.run(&[
            "outline",
            "generate",
            "--course",
            &id,
            "--topic",
            "Cell membranes",
        ])
        .ok::<OutlineSummary>();
        id
    }

    /// A course with an approved outline and every chapter written.
    fn written(&self) -> String {
        let id = self.drafted();
        self.run(&["outline", "approve", "--course", &id, "--generate", "all"])
            .ok::<Outline>();
        id
    }

    /// The kinds of fake-claude job run so far, in order.
    fn job_kinds(&self) -> Vec<String> {
        let log = std::fs::read_to_string(self.state.join("calls.log")).unwrap_or_default();
        log.lines()
            .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
            .collect()
    }

    fn calls_log(&self) -> String {
        std::fs::read_to_string(self.state.join("calls.log")).unwrap_or_default()
    }
}

fn write_json(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

// ---- courses, course create, templates ----------------------------------

#[test]
fn courses_lists_the_library_and_create_adds_to_it() {
    let env = Env::new();
    assert!(env.run(&["courses"]).ok::<Vec<CourseSummary>>().is_empty());
    let created: CourseSummary = env
        .run(&[
            "course",
            "create",
            "Cell Membranes",
            "--tag",
            "biology",
            "--tag",
            "exam",
        ])
        .ok();
    assert_eq!(created.title, "Cell Membranes");
    assert_eq!(created.tags, vec!["biology", "exam"]);
    let listed: Vec<CourseSummary> = env.run(&["courses"]).ok();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);
    assert!(env.course_dir(&created.id).join("course.json").is_file());
}

#[test]
fn courses_refuses_a_folder_that_is_not_a_library() {
    let env = Env::new();
    std::fs::create_dir_all(&env.library).unwrap();
    std::fs::write(env.library.join("notes.txt"), "mine").unwrap();
    let reason = env.run(&["courses"]).refused();
    assert!(reason.contains("not a TutorDocs library"), "{reason}");
    assert!(!env.library.join("course.json").exists());
}

#[test]
fn course_create_refuses_an_empty_title() {
    let env = Env::new();
    let reason = env.run(&["course", "create", "   "]).refused();
    assert!(reason.to_lowercase().contains("title"), "{reason}");
}

#[test]
fn the_library_flag_beats_the_environment_variable() {
    let env = Env::new();
    let other = env.tmp.path().join("other-library");
    let out = env.run(&[
        "--library",
        other.to_str().unwrap(),
        "course",
        "create",
        "Elsewhere",
    ]);
    out.ok::<CourseSummary>();
    assert!(other.join("courses").is_dir());
    assert!(!env.library.exists(), "the env library must stay untouched");
}

#[test]
fn without_a_flag_or_variable_the_library_falls_back_to_home_tutordocs() {
    let env = Env::new();
    let mut command = env.command(&["course", "create", "Home course"]);
    command.env_remove("TUTORDOCS_LIBRARY");
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(env.home.join("TutorDocs/courses").is_dir());
}

#[test]
fn templates_lists_the_four_built_in_templates() {
    let env = Env::new();
    let templates: Vec<Template> = env.run(&["templates"]).ok();
    let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["interview", "exam", "overview", "custom"]);
}

#[test]
fn an_unknown_argument_is_a_usage_error() {
    let env = Env::new();
    env.run(&["templates", "--nonsense"]).usage_error();
    env.run(&["no-such-command"]).usage_error();
    env.run(&[]).usage_error();
}

// ---- course setup --------------------------------------------------------

#[test]
fn setup_set_then_get_round_trips_through_flags() {
    let env = Env::new();
    let id = env.course();
    assert_eq!(
        env.run(&["course", "setup", "get", "--course", &id])
            .value(),
        Value::Null
    );
    let set: CourseSetup = env
        .run(&[
            "course",
            "setup",
            "set",
            "--course",
            &id,
            "--template",
            "exam",
            "--level",
            "some",
            "--hours",
            "4",
            "--language",
            "en",
            "--guide",
            "focus on transport",
        ])
        .ok();
    assert_eq!(set.template, "exam");
    assert_eq!(set.hours_per_week, Some(4));
    let got: Option<CourseSetup> = env.run(&["course", "setup", "get", "--course", &id]).ok();
    assert_eq!(got, Some(set));
}

#[test]
fn setup_set_reads_a_file() {
    let env = Env::new();
    let id = env.course();
    let file = env.tmp.path().join("setup.json");
    write_json(
        &file,
        &json!({"template": "interview", "level": "new", "hoursPerWeek": null,
                "language": "th", "guide": ""}),
    );
    let set: CourseSetup = env
        .run(&[
            "course",
            "setup",
            "set",
            "--course",
            &id,
            "--file",
            file.to_str().unwrap(),
        ])
        .ok();
    assert_eq!(set.language, "th");
    assert_eq!(set.hours_per_week, None);
}

#[test]
fn setup_set_refuses_an_unknown_template_and_an_unreadable_file() {
    let env = Env::new();
    let id = env.course();
    let reason = env
        .run(&[
            "course",
            "setup",
            "set",
            "--course",
            &id,
            "--template",
            "bogus",
            "--level",
            "new",
            "--language",
            "en",
        ])
        .refused();
    assert!(reason.contains("unknown course template"), "{reason}");
    let reason = env
        .run(&[
            "course",
            "setup",
            "set",
            "--course",
            &id,
            "--file",
            "/no/such/setup.json",
        ])
        .refused();
    assert!(reason.contains("setup.json"), "{reason}");
    assert_eq!(
        env.run(&["course", "setup", "get", "--course", &id])
            .value(),
        Value::Null,
        "a refused setup writes nothing"
    );
}

#[test]
fn setup_set_needs_either_flags_or_a_file() {
    let env = Env::new();
    let id = env.course();
    env.run(&["course", "setup", "set", "--course", &id])
        .usage_error();
    env.run(&[
        "course",
        "setup",
        "set",
        "--course",
        &id,
        "--file",
        "x.json",
        "--template",
        "exam",
    ])
    .usage_error();
}

#[test]
fn setup_get_refuses_an_unknown_course() {
    let env = Env::new();
    let reason = env
        .run(&["course", "setup", "get", "--course", "nope"])
        .refused();
    assert!(reason.contains("nope"), "{reason}");
}

// ---- import, sources -----------------------------------------------------

#[test]
fn import_copies_supported_files_and_skips_the_rest_and_sources_lists_them() {
    let env = Env::new();
    let created: CourseSummary = env.run(&["course", "create", "Cell Membranes"]).ok();
    let id = created.id;
    let (notes, slides) = (env.input("study-notes.md"), env.input("slides.docx"));
    let results: Vec<ImportResult> = env.run(&["import", "--course", &id, &notes, &slides]).ok();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].source.as_ref().unwrap().name, "study-notes.md");
    assert!(
        results[1].reason.is_some(),
        "an unsupported file is skipped with a reason"
    );

    let again: Vec<ImportResult> = env.run(&["import", "--course", &id, &notes]).ok();
    assert_eq!(again[0].reason.as_deref(), Some("duplicate"));

    let sources: Vec<CourseSource> = env.run(&["sources", "--course", &id]).ok();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].name, "study-notes.md");
    assert!(env.course_dir(&id).join("sources/study-notes.md").is_file());
    assert!(
        env.inputs.join("study-notes.md").is_file(),
        "the original stays"
    );
}

#[test]
fn import_and_sources_refuse_an_unknown_course() {
    let env = Env::new();
    let notes = env.input("study-notes.md");
    assert!(env
        .run(&["import", "--course", "nope", &notes])
        .refused()
        .contains("nope"));
    assert!(env
        .run(&["sources", "--course", "nope"])
        .refused()
        .contains("nope"));
}

#[test]
fn import_needs_at_least_one_path() {
    let env = Env::new();
    let id = env.course();
    env.run(&["import", "--course", &id]).usage_error();
}

// ---- outline -------------------------------------------------------------

#[test]
fn outline_generate_writes_a_draft_and_streams_the_job_to_stderr() {
    let env = Env::new();
    let id = env.course();
    let out = env.run(&[
        "outline",
        "generate",
        "--course",
        &id,
        "--topic",
        "Cell membranes",
        "--source",
        "study-notes.md",
        "--source",
        "glossary.txt",
    ]);
    let summary: OutlineSummary = out.ok();
    assert_eq!(summary.title, "Cell Membranes");
    assert_eq!(summary.chapter_ids.len(), 4);

    let lines = out.job_lines();
    assert!(
        !lines.is_empty(),
        "a job streams its lines\n{}",
        out.context()
    );
    for line in &lines {
        assert!(
            line["jobId"].as_str().unwrap().starts_with("job-"),
            "{line}"
        );
        assert!(line["line"]["at"].is_string(), "{line}");
        assert!(line["line"]["text"].is_string(), "{line}");
        assert!(
            ["text", "tool", "error", "status"].contains(&line["line"]["kind"].as_str().unwrap()),
            "{line}"
        );
    }
    let texts: Vec<&str> = lines
        .iter()
        .filter_map(|l| l["line"]["text"].as_str())
        .collect();
    assert_eq!(texts.first(), Some(&"running"));
    assert_eq!(texts.last(), Some(&"succeeded"));

    let shown: Outline = env.run(&["outline", "show", "--course", &id]).ok();
    assert_eq!(shown.status, tutordocs_store::OutlineStatus::Draft);
    assert_eq!(env.job_kinds(), ["outline"]);
    assert!(
        env.calls_log().contains("anthropic_key=unset"),
        "{}",
        env.calls_log()
    );
}

#[test]
fn outline_generate_refuses_before_any_job_for_an_unknown_course_or_agent() {
    let env = Env::new();
    let id = env.course();
    let reason = env
        .run(&["outline", "generate", "--course", "nope"])
        .refused();
    assert!(reason.contains("nope"), "{reason}");
    let reason = env
        .run(&[
            "outline", "generate", "--course", &id, "--topic", "x", "--agent", "bogus",
        ])
        .refused();
    assert!(reason.contains("bogus"), "{reason}");
    assert!(env.job_kinds().is_empty(), "a refusal runs no job");
}

#[test]
fn agent_model_and_effort_flags_reach_the_agent_cli() {
    let env = Env::new();
    let id = env.course();
    env.run(&[
        "outline", "generate", "--course", &id, "--topic", "x", "--agent", "claude", "--model",
        "sonnet", "--effort", "low",
    ])
    .ok::<OutlineSummary>();
    let log = env.calls_log();
    assert!(log.contains("sonnet") && log.contains("low"), "{log}");
}

#[test]
fn outline_show_and_save_round_trip_and_refuse_a_written_chapter_removal() {
    let env = Env::new();
    let id = env.drafted();
    let mut outline: Value = env.run(&["outline", "show", "--course", &id]).value();
    assert_eq!(outline["status"], "draft");
    assert_eq!(outline["chapters"].as_array().unwrap().len(), 4);
    outline["chapters"][1]["comment"] = json!("add a worked example");
    outline["chapters"][1]["title"] = json!("Passive transport, reworded");
    let file = env.tmp.path().join("outline.json");
    write_json(&file, &outline);

    let saved: Outline = env
        .run(&["outline", "save", "--course", &id, file.to_str().unwrap()])
        .ok();
    assert_eq!(saved.chapters[1].comment, "add a worked example");
    assert_eq!(saved.chapters[1].title, "Passive transport, reworded");
    let again: Outline = env
        .run(&["outline", "save", "--course", &id, file.to_str().unwrap()])
        .ok();
    assert_eq!(again, saved, "saving twice gives the same outline");

    // Approve, write one chapter, then try to drop it.
    env.run(&["outline", "approve", "--course", &id])
        .ok::<Outline>();
    env.run(&[
        "chapter",
        "open",
        "--course",
        &id,
        "--chapter",
        "membrane-structure",
    ])
    .ok::<ChapterDoc>();
    let mut outline: Value = env.run(&["outline", "show", "--course", &id]).value();
    outline["chapters"].as_array_mut().unwrap().remove(0);
    write_json(&file, &outline);
    let reason = env
        .run(&["outline", "save", "--course", &id, file.to_str().unwrap()])
        .refused();
    assert!(reason.contains("written"), "{reason}");
}

#[test]
fn outline_save_refuses_a_missing_or_malformed_file() {
    let env = Env::new();
    let id = env.drafted();
    let reason = env
        .run(&["outline", "save", "--course", &id, "/no/such/outline.json"])
        .refused();
    assert!(reason.contains("outline.json"), "{reason}");
    let bad = env.tmp.path().join("bad.json");
    std::fs::write(&bad, "{\"title\": 3}").unwrap();
    let reason = env
        .run(&["outline", "save", "--course", &id, bad.to_str().unwrap()])
        .refused();
    assert!(reason.contains("bad.json"), "{reason}");
}

#[test]
fn outline_show_refuses_an_unknown_course() {
    let env = Env::new();
    assert!(env
        .run(&["outline", "show", "--course", "nope"])
        .refused()
        .contains("nope"));
}

#[test]
fn outline_revise_applies_comments_and_clears_them() {
    let env = Env::new();
    let id = env.drafted();
    let mut outline: Value = env.run(&["outline", "show", "--course", &id]).value();
    outline["chapters"][2]["comment"] = json!("cover the sodium potassium pump");
    let file = env.tmp.path().join("outline.json");
    write_json(&file, &outline);
    env.run(&["outline", "save", "--course", &id, file.to_str().unwrap()])
        .ok::<Outline>();

    let out = env.run(&["outline", "revise", "--course", &id]);
    let revised: Outline = out.ok();
    assert!(
        revised.chapters[2].title.ends_with("(revised)"),
        "{:?}",
        revised.chapters[2]
    );
    assert!(revised.chapters.iter().all(|c| c.comment.is_empty()));
    assert_eq!(
        revised.status,
        tutordocs_store::OutlineStatus::Draft,
        "status stays"
    );
    assert!(!out.job_lines().is_empty());
    assert_eq!(env.job_kinds(), ["outline", "revise_outline"]);
}

#[test]
fn outline_revise_refuses_an_unknown_course() {
    let env = Env::new();
    assert!(env
        .run(&["outline", "revise", "--course", "nope"])
        .refused()
        .contains("nope"));
}

#[test]
fn outline_approve_on_open_approves_without_running_a_job() {
    let env = Env::new();
    let id = env.drafted();
    let approved: Outline = env
        .run(&[
            "outline",
            "approve",
            "--course",
            &id,
            "--generate",
            "on_open",
        ])
        .ok();
    assert_eq!(approved.status, tutordocs_store::OutlineStatus::Approved);
    assert!(approved.chapters.iter().all(|c| !c.written));
    assert_eq!(env.job_kinds(), ["outline"], "only the outline job ran");
}

#[test]
fn outline_approve_all_writes_every_chapter_in_order_then_does_nothing_new() {
    let env = Env::new();
    let id = env.drafted();
    let out = env.run(&["outline", "approve", "--course", &id, "--generate", "all"]);
    let approved: Outline = out.ok();
    assert_eq!(approved.status, tutordocs_store::OutlineStatus::Approved);
    assert!(approved.chapters.iter().all(|c| c.written), "{approved:?}");
    for chapter in &approved.chapters {
        let path = env
            .course_dir(&id)
            .join("chapters")
            .join(chapter.id.as_ref().unwrap());
        assert!(path.join("index.md").is_file(), "{}", path.display());
    }
    assert_eq!(
        env.job_kinds(),
        ["outline", "chapter", "chapter", "chapter", "chapter"]
    );
    let ids: std::collections::BTreeSet<String> = out
        .job_lines()
        .iter()
        .map(|l| l["jobId"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids.len(), 4, "four chapter jobs streamed: {ids:?}");

    let again: Outline = env
        .run(&["outline", "approve", "--course", &id, "--generate", "all"])
        .ok();
    assert_eq!(again, approved);
    assert_eq!(env.job_kinds().len(), 5, "approving again runs no job");
}

#[test]
fn outline_approve_refuses_an_unknown_course_and_a_bad_generate_value() {
    let env = Env::new();
    assert!(env
        .run(&[
            "outline",
            "approve",
            "--course",
            "nope",
            "--generate",
            "all"
        ])
        .refused()
        .contains("nope"));
    let id = env.drafted();
    env.run(&[
        "outline",
        "approve",
        "--course",
        &id,
        "--generate",
        "sometimes",
    ])
    .usage_error();
    assert!(env.job_kinds() == ["outline"]);
}

#[test]
fn outline_variants_branch_variant_show_and_approve_promotes() {
    let env = Env::new();
    let id = env.drafted();

    // 1. Initial variants list shows "current"
    let list: Vec<VariantMeta> = env.run(&["outline", "variants", "--course", &id]).ok();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "current");
    assert_eq!(list[0].label, "Current");
    assert_eq!(list[0].chapter_count, 4);
    assert_eq!(list[0].written_chapter_count, 0);

    // 2. variant-show current
    let current_variant: OutlineVariant = env
        .run(&["outline", "variant-show", "--course", &id, "current"])
        .ok();
    assert_eq!(current_variant.id, "current");
    assert_eq!(current_variant.label, "Current");
    assert_eq!(current_variant.chapters.len(), 4);

    // 3. branch to Draft B
    let draft_b: OutlineVariant = env
        .run(&["outline", "branch", "--course", &id, "--label", "Draft B"])
        .ok();
    assert_eq!(draft_b.id, "draft-b");
    assert_eq!(draft_b.label, "Draft B");
    assert_eq!(draft_b.parent_id.as_deref(), Some("current"));
    assert_eq!(draft_b.chapters.len(), 4);

    // 4. variants list now shows 2 variants
    let list2: Vec<VariantMeta> = env.run(&["outline", "variants", "--course", &id]).ok();
    assert_eq!(list2.len(), 2);
    assert_eq!(list2[0].id, "current");
    assert_eq!(list2[1].id, "draft-b");

    // 5. approve on_open and write chapter 1
    env.run(&["outline", "approve", "--course", &id])
        .ok::<Outline>();
    env.run(&[
        "chapter",
        "open",
        "--course",
        &id,
        "--chapter",
        "membrane-structure",
    ])
    .ok::<ChapterDoc>();

    // 6. saving draft-b while removing written chapter is refused
    let mut modified = Outline {
        status: tutordocs_store::OutlineStatus::Draft,
        title: "Cell Membranes".to_string(),
        objectives: current_variant.objectives.clone(),
        chapters: current_variant.chapters.clone(),
    };
    modified
        .chapters
        .retain(|c| c.id.as_deref() != Some("membrane-structure"));
    let outline_file = env.tmp.path().join("modified_variant.json");
    write_json(&outline_file, &serde_json::to_value(&modified).unwrap());
    let reason = env
        .run(&[
            "outline",
            "save",
            "--course",
            &id,
            outline_file.to_str().unwrap(),
            "--variant",
            "draft-b",
        ])
        .refused();
    assert!(reason.contains("cannot remove written chapter"), "{reason}");

    // 7. promote draft-b with approve --variant draft-b
    let promoted: Outline = env
        .run(&[
            "outline",
            "approve",
            "--course",
            &id,
            "--variant",
            "draft-b",
        ])
        .ok();
    assert_eq!(promoted.status, tutordocs_store::OutlineStatus::Approved);

    // Now draft-b is in course.json (current), and outgoing current is in outlines/current.json
    let list3: Vec<VariantMeta> = env.run(&["outline", "variants", "--course", &id]).ok();
    assert_eq!(list3.len(), 2);
    let curr = list3.iter().find(|v| v.id == "draft-b").unwrap();
    assert_eq!(curr.label, "Draft B");
    assert!(curr.current);
    let other = list3.iter().find(|v| v.id == "current").unwrap();
    assert!(!other.current);
}

#[test]
fn ctrl_c_stops_the_queue_after_the_running_job_and_exits_130() {
    let env = Env::new();
    let id = env.drafted();
    let mut child = env
        .command(&["outline", "approve", "--course", &id, "--generate", "all"])
        .env("FAKE_PAUSE", "0.4")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    // Wait for the first chapter job to start, then interrupt.
    let first = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("a job line arrives");
    assert!(first.contains("running"), "{first}");
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let output = child.wait_with_output().unwrap();
    reader.join().unwrap();
    assert_eq!(
        output.status.code(),
        Some(130),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert!(
        value["error"].as_str().unwrap().contains("interrupted"),
        "{value}"
    );

    let outline: Outline = env.run(&["outline", "show", "--course", &id]).ok();
    let written = outline.chapters.iter().filter(|c| c.written).count();
    assert_eq!(
        written, 1,
        "the running job finished, the rest never started: {outline:?}"
    );
    assert_eq!(
        env.job_kinds().len(),
        2,
        "outline plus one chapter: {:?}",
        env.job_kinds()
    );
}

// ---- queue ---------------------------------------------------------------

#[test]
fn queue_reports_an_idle_queue_and_refuses_an_unknown_course() {
    let env = Env::new();
    let id = env.drafted();
    let queue: ChapterQueue = env.run(&["queue", "--course", &id]).ok();
    assert_eq!(
        queue,
        ChapterQueue {
            running: None,
            pending: vec![]
        }
    );
    assert!(env
        .run(&["queue", "--course", "nope"])
        .refused()
        .contains("nope"));
}

// ---- chapter -------------------------------------------------------------

#[test]
fn chapter_list_shows_placeholders_until_a_chapter_is_opened() {
    let env = Env::new();
    let id = env.drafted();
    env.run(&["outline", "approve", "--course", &id])
        .ok::<Outline>();
    let before: Vec<ChapterSummary> = env.run(&["chapter", "list", "--course", &id]).ok();
    assert_eq!(before.len(), 4);
    assert!(before.iter().all(|c| c.placeholder));

    let out = env.run(&[
        "chapter",
        "open",
        "--course",
        &id,
        "--chapter",
        "passive-transport",
    ]);
    let doc: Value = out.value();
    assert_eq!(doc["id"], "passive-transport");
    assert_eq!(doc["state"], "ready");
    assert!(doc["markdown"]
        .as_str()
        .unwrap()
        .contains("## The core idea"));
    assert!(!out.job_lines().is_empty());

    let after: Vec<ChapterSummary> = env.run(&["chapter", "list", "--course", &id]).ok();
    let placeholders: Vec<bool> = after.iter().map(|c| c.placeholder).collect();
    assert_eq!(placeholders, [true, false, true, true]);

    // Opening a written chapter again returns it with no new job.
    let jobs_before = env.job_kinds().len();
    env.run(&[
        "chapter",
        "open",
        "--course",
        &id,
        "--chapter",
        "passive-transport",
    ])
    .ok::<ChapterDoc>();
    assert_eq!(env.job_kinds().len(), jobs_before);
}

#[test]
fn chapter_open_refuses_a_draft_outline_and_an_unknown_chapter() {
    let env = Env::new();
    let id = env.drafted();
    let reason = env
        .run(&[
            "chapter",
            "open",
            "--course",
            &id,
            "--chapter",
            "passive-transport",
        ])
        .refused();
    assert_eq!(reason, "outline is a draft");
    env.run(&["outline", "approve", "--course", &id])
        .ok::<Outline>();
    let reason = env
        .run(&["chapter", "open", "--course", &id, "--chapter", "nope"])
        .refused();
    assert!(reason.contains("nope"), "{reason}");
    assert!(env
        .run(&["chapter", "list", "--course", "nope"])
        .refused()
        .contains("nope"));
}

#[test]
fn chapter_rewrite_writes_the_chapter_again() {
    let env = Env::new();
    let id = env.written();
    let jobs_before = env.job_kinds().len();
    let out = env.run(&[
        "chapter",
        "rewrite",
        "--course",
        &id,
        "--chapter",
        "cell-signaling",
    ]);
    let doc: ChapterDoc = out.ok();
    assert_eq!(doc.id, "cell-signaling");
    assert!(!out.job_lines().is_empty());
    assert_eq!(env.job_kinds().len(), jobs_before + 1);
    assert_eq!(env.job_kinds().last().map(String::as_str), Some("chapter"));
}

#[test]
fn chapter_rewrite_refuses_an_unknown_chapter() {
    let env = Env::new();
    let id = env.written();
    let reason = env
        .run(&["chapter", "rewrite", "--course", &id, "--chapter", "nope"])
        .refused();
    assert!(reason.contains("nope"), "{reason}");
}

// ---- section revise, ask, answers, context --------------------------------

#[test]
fn section_revise_replaces_only_the_section_holding_the_quote() {
    let env = Env::new();
    let id = env.written();
    let path = env
        .course_dir(&id)
        .join("chapters/passive-transport/index.md");
    let before = std::fs::read_to_string(&path).unwrap();
    let out = env.run(&[
        "section",
        "revise",
        "--course",
        &id,
        "--chapter",
        "passive-transport",
        "--quote",
        "Oxygen is small",
        "--instruction",
        "say it in plainer words",
    ]);
    let doc: Value = out.value();
    assert_eq!(doc["state"], "ready");
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(doc["markdown"], after);
    assert!(after.contains("REVISED SECTION"), "{after}");
    assert!(after.contains("say it in plainer words"), "{after}");
    assert!(after.contains("## The core idea"), "other sections stay");
    assert!(before.contains("## Worked example") && after.contains("## Worked example"));
    assert!(
        !after.contains("Oxygen is small"),
        "the old section text is gone"
    );
    assert!(!out.job_lines().is_empty());
    assert_eq!(
        env.job_kinds().last().map(String::as_str),
        Some("revise_section")
    );
}

#[test]
fn section_revise_refuses_an_empty_instruction_and_an_unwritten_chapter() {
    let env = Env::new();
    let id = env.drafted();
    env.run(&["outline", "approve", "--course", &id])
        .ok::<Outline>();
    let reason = env
        .run(&[
            "section",
            "revise",
            "--course",
            &id,
            "--chapter",
            "passive-transport",
            "--quote",
            "x",
            "--instruction",
            "fix it",
        ])
        .refused();
    assert!(reason.contains("not written"), "{reason}");
    let id = env.written();
    let reason = env
        .run(&[
            "section",
            "revise",
            "--course",
            &id,
            "--chapter",
            "passive-transport",
            "--quote",
            "Oxygen",
            "--instruction",
            "  ",
        ])
        .refused();
    assert!(reason.contains("instruction"), "{reason}");
}

#[test]
fn section_revise_needs_a_quote_and_an_instruction() {
    let env = Env::new();
    let id = env.written();
    env.run(&[
        "section",
        "revise",
        "--course",
        &id,
        "--chapter",
        "passive-transport",
    ])
    .usage_error();
}

#[test]
fn ask_stores_an_answer_that_answers_and_context_then_show() {
    let env = Env::new();
    let id = env.written();
    let out = env.run(&[
        "ask",
        "--course",
        &id,
        "--chapter",
        "passive-transport",
        "--quote",
        "Oxygen is small",
        "Why does oxygen cross on its own?",
    ]);
    let answer: ChapterAnswer = out.ok();
    assert_eq!(answer.question, "Why does oxygen cross on its own?");
    assert_eq!(answer.quote.as_deref(), Some("Oxygen is small"));
    assert!(answer.answer.contains("selective") || answer.answer.contains("membrane"));
    assert!(!out.job_lines().is_empty());

    env.run(&[
        "ask",
        "--course",
        &id,
        "--chapter",
        "passive-transport",
        "And glucose?",
    ])
    .ok::<ChapterAnswer>();
    let answers: Vec<ChapterAnswer> = env
        .run(&["answers", "--course", &id, "--chapter", "passive-transport"])
        .ok();
    assert_eq!(answers.len(), 2);
    assert_eq!(answers[0].question, "And glucose?", "newest first");
    assert_eq!(answers[0].quote, None);

    let context: ChapterContext = env
        .run(&["context", "--course", &id, "--chapter", "passive-transport"])
        .ok();
    assert_eq!(context.objectives.len(), 2);
    assert!(context.summary.contains("Diffusion"));
}

#[test]
fn ask_answers_and_context_refuse_bad_input() {
    let env = Env::new();
    let id = env.written();
    let reason = env
        .run(&[
            "ask",
            "--course",
            &id,
            "--chapter",
            "passive-transport",
            "   ",
        ])
        .refused();
    assert!(reason.contains("question"), "{reason}");
    let reason = env
        .run(&["ask", "--course", &id, "--chapter", "nope", "Why?"])
        .refused();
    assert!(reason.contains("nope"), "{reason}");
    assert!(env
        .run(&["answers", "--course", &id, "--chapter", "nope"])
        .refused()
        .contains("nope"));
    assert!(env
        .run(&["context", "--course", &id, "--chapter", "nope"])
        .refused()
        .contains("nope"));
    env.run(&["ask", "--course", &id, "--chapter", "passive-transport"])
        .usage_error();
}

// ---- the whole A to D backend flow ---------------------------------------

#[test]
fn the_whole_a_to_d_flow_runs_through_the_cli_in_order() {
    let env = Env::new();

    // C: guided creation.
    let created: CourseSummary = env
        .run(&["course", "create", "Cell Membranes", "--tag", "biology"])
        .ok();
    let id = created.id;
    let templates: Vec<Template> = env.run(&["templates"]).ok();
    assert_eq!(templates[0].id, "interview");
    env.run(&[
        "course",
        "setup",
        "set",
        "--course",
        &id,
        "--template",
        "exam",
        "--level",
        "some",
        "--hours",
        "3",
        "--language",
        "en",
        "--guide",
        "focus on transport",
    ])
    .ok::<CourseSetup>();

    // C: import files.
    let (notes, glossary, slides) = (
        env.input("study-notes.md"),
        env.input("glossary.txt"),
        env.input("slides.docx"),
    );
    let imports: Vec<ImportResult> = env
        .run(&["import", "--course", &id, &notes, &glossary, &slides])
        .ok();
    assert_eq!(imports.len(), 3);
    let sources: Vec<CourseSource> = env.run(&["sources", "--course", &id]).ok();
    assert_eq!(sources.len(), 2);

    // A: draft, review, comment, revise, approve and write everything.
    env.run(&[
        "outline",
        "generate",
        "--course",
        &id,
        "--topic",
        "Cell membranes",
        "--source",
        "study-notes.md",
        "--source",
        "glossary.txt",
    ])
    .ok::<OutlineSummary>();
    let mut outline: Value = env.run(&["outline", "show", "--course", &id]).value();
    assert_eq!(outline["status"], "draft");
    outline["chapters"][1]["comment"] = json!("add a worked example");
    let file = env.tmp.path().join("outline.json");
    write_json(&file, &outline);
    env.run(&["outline", "save", "--course", &id, file.to_str().unwrap()])
        .ok::<Outline>();
    let revised: Outline = env.run(&["outline", "revise", "--course", &id]).ok();
    assert!(revised.chapters[1].title.ends_with("(revised)"));
    let approved: Outline = env
        .run(&["outline", "approve", "--course", &id, "--generate", "all"])
        .ok();
    assert!(approved.chapters.iter().all(|c| c.written));

    // B: revise one section.
    env.run(&[
        "section",
        "revise",
        "--course",
        &id,
        "--chapter",
        "passive-transport",
        "--quote",
        "Oxygen is small",
        "--instruction",
        "plainer words",
    ])
    .ok::<ChapterDoc>();

    // D: ask, then read the panel's data.
    env.run(&[
        "ask",
        "--course",
        &id,
        "--chapter",
        "passive-transport",
        "--quote",
        "Oxygen is small",
        "Why?",
    ])
    .ok::<ChapterAnswer>();
    let answers: Vec<ChapterAnswer> = env
        .run(&["answers", "--course", &id, "--chapter", "passive-transport"])
        .ok();
    assert_eq!(answers.len(), 1);
    let context: ChapterContext = env
        .run(&["context", "--course", &id, "--chapter", "passive-transport"])
        .ok();
    assert!(!context.objectives.is_empty());

    // The files on disk.
    let course_dir = env.course_dir(&id);
    let course: Value =
        serde_json::from_str(&std::fs::read_to_string(course_dir.join("course.json")).unwrap())
            .unwrap();
    assert_eq!(course["outlineStatus"], "approved");
    assert_eq!(course["setup"]["template"], "exam");
    assert_eq!(course["chapters"].as_array().unwrap().len(), 4);
    for chapter in course["chapters"].as_array().unwrap() {
        let chapter_id = chapter["id"].as_str().unwrap();
        let index = course_dir
            .join("chapters")
            .join(chapter_id)
            .join("index.md");
        assert!(index.is_file(), "{}", index.display());
        assert_eq!(
            chapter["comment"].as_str().unwrap_or(""),
            "",
            "comments were cleared"
        );
    }
    let revised_chapter =
        std::fs::read_to_string(course_dir.join("chapters/passive-transport/index.md")).unwrap();
    assert!(revised_chapter.contains("REVISED SECTION"));
    assert!(revised_chapter.contains("## The core idea"));
    assert!(course_dir.join("sources/study-notes.md").is_file());
    assert!(course_dir.join("sources/glossary.txt").is_file());
    assert!(!course_dir.join("sources/slides.docx").exists());
    assert!(course_dir.join("store.sqlite").is_file() || course_dir.join(".tutordocs").exists());

    assert_eq!(
        env.job_kinds(),
        [
            "outline",
            "revise_outline",
            "chapter",
            "chapter",
            "chapter",
            "chapter",
            "revise_section",
            "ask"
        ]
    );
    let log = env.calls_log();
    assert_eq!(log.matches("anthropic_key=unset").count(), 8, "{log}");
    assert!(
        !env.home.join("TutorDocs").exists(),
        "the home library stays untouched"
    );
}

#[test]
fn course_trash_and_restore_cycle() {
    let env = Env::new();
    let created: CourseSummary = env.run(&["course", "create", "Cell Membranes"]).ok();
    assert_eq!(created.id, "cell-membranes");

    let trashed: TrashEntry = env.run(&["course", "trash", &created.id]).ok();
    assert_eq!(trashed.course_id, "cell-membranes");
    assert_eq!(trashed.title, "Cell Membranes");
    assert!(trashed.trash_id.starts_with("cell-membranes-"));

    let trash_list: Vec<TrashEntry> = env.run(&["course", "trash-list"]).ok();
    assert_eq!(trash_list.len(), 1);
    assert_eq!(trash_list[0].trash_id, trashed.trash_id);

    let courses: Vec<CourseSummary> = env.run(&["courses"]).ok();
    assert!(courses.is_empty(), "trashed course must not be listed");

    let restored: CourseSummary = env.run(&["course", "restore", &trashed.trash_id]).ok();
    assert_eq!(restored.id, "cell-membranes");
    assert_eq!(restored.title, "Cell Membranes");

    let courses_after: Vec<CourseSummary> = env.run(&["courses"]).ok();
    assert_eq!(courses_after.len(), 1);
    assert_eq!(courses_after[0].id, "cell-membranes");

    let trash_list_after: Vec<TrashEntry> = env.run(&["course", "trash-list"]).ok();
    assert!(trash_list_after.is_empty());
}

#[test]
fn course_trash_empty_permanently_removes() {
    let env = Env::new();
    let created: CourseSummary = env.run(&["course", "create", "Cell Membranes"]).ok();

    let trashed: TrashEntry = env.run(&["course", "trash", &created.id]).ok();
    let trash_list: Vec<TrashEntry> = env.run(&["course", "trash-list"]).ok();
    assert_eq!(trash_list.len(), 1);

    env.run(&["course", "trash-empty", &trashed.trash_id])
        .ok::<Value>();

    let trash_list_after: Vec<TrashEntry> = env.run(&["course", "trash-list"]).ok();
    assert!(trash_list_after.is_empty());
}

#[test]
fn course_restore_refuses_when_id_taken() {
    let env = Env::new();
    let created: CourseSummary = env.run(&["course", "create", "Biology"]).ok();
    let trashed: TrashEntry = env.run(&["course", "trash", &created.id]).ok();

    // Recreate the course
    env.run(&["course", "create", "Biology"])
        .ok::<CourseSummary>();

    let reason = env.run(&["course", "restore", &trashed.trash_id]).refused();
    assert!(
        reason.contains("biology"),
        "error should name biology, got: {reason}"
    );
}

#[test]
fn course_rename_changes_title_and_keeps_folder() {
    let env = Env::new();
    let created: CourseSummary = env.run(&["course", "create", "Cell Biology"]).ok();
    assert_eq!(created.id, "cell-biology");

    let renamed: CourseSummary = env
        .run(&["course", "rename", "cell-biology", "Advanced Cell Biology"])
        .ok();
    assert_eq!(renamed.id, "cell-biology");
    assert_eq!(renamed.title, "Advanced Cell Biology");

    let courses: Vec<CourseSummary> = env.run(&["courses"]).ok();
    assert_eq!(courses[0].title, "Advanced Cell Biology");
    assert_eq!(courses[0].id, "cell-biology");

    let reason_empty = env
        .run(&["course", "rename", "cell-biology", "   "])
        .refused();
    assert!(
        reason_empty.contains("title"),
        "expected title refusal, got: {reason_empty}"
    );

    let reason_missing = env
        .run(&["course", "rename", "missing", "New Title"])
        .refused();
    assert!(
        reason_missing.contains("no course") || reason_missing.contains("missing"),
        "got: {reason_missing}"
    );
}
