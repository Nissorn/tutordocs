//! Quiz commands: write a chapter's quiz, grade the learner's free-text
//! answers, and report progress per course objective. The engine does the work
//! (`tutordocs_engine`); grades persist in the library's mastery log, so
//! progress survives a restart.

use tauri::State;
use tutordocs_engine::{
    course_progress as engine_progress, generate_quiz as engine_quiz,
    grade_answers as engine_grade, job_scope, unix_now, Answer, GradedAnswer, ObjectiveProgress,
    Quiz,
};

use crate::generate::GenerationState;
use crate::jobs::JobRegistry;

#[tauri::command]
pub async fn generate_quiz(
    state: State<'_, GenerationState>,
    jobs: State<'_, JobRegistry>,
    chapter_id: String,
) -> Result<Quiz, String> {
    let driver = state.driver().await?;
    let root = crate::reader::course_dir(state.root(), None)?;
    job_scope(jobs.log(), engine_quiz(&root, driver.as_ref(), &chapter_id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn grade_quiz(
    state: State<'_, GenerationState>,
    jobs: State<'_, JobRegistry>,
    chapter_id: String,
    answers: Vec<Answer>,
) -> Result<Vec<GradedAnswer>, String> {
    let driver = state.driver().await?;
    let root = crate::reader::course_dir(state.root(), None)?;
    job_scope(
        jobs.log(),
        engine_grade(&root, driver.as_ref(), &chapter_id, &answers, unix_now()),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn course_progress(
    state: State<'_, GenerationState>,
) -> Result<Vec<ObjectiveProgress>, String> {
    let root = crate::reader::course_dir(state.root(), None)?;
    engine_progress(&root, unix_now()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tutordocs_store::{ChapterRef, Course, Library};

    use crate::generate::GenerationState;
    use crate::reader::{install, ReaderState};

    /// The wire shapes both runtimes agree on; vitest replays the same file.
    fn contract() -> Value {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ui/src/fixtures/quiz-contract.json"
        ));
        serde_json::from_str(text).expect("contract fixture is valid JSON")
    }

    /// A fake `claude`: a quiz job answers with `quiz.json`, a grading job with
    /// `verdicts.json`, both beside the script. Every job's argv is logged.
    const FAKE: &str = r#"#!/bin/sh
case "$1" in
  --version) echo "2.1.319 (Claude Code)"; exit 0 ;;
  --help) printf '%s\n' "-p, --print" "--output-format stream-json" "--json-schema" "--resume" "--append-system-prompt" "--tools" "--allowedTools" "--permission-mode"; exit 0 ;;
esac
HERE=$(dirname "$0")
printf '[%s]' "$@" | tr '\n' ' ' >> "$HERE/jobs.log"
echo >> "$HERE/jobs.log"
case "$2" in
  *"Write a short quiz"*) OUT="$HERE/quiz.json" ;;
  *) OUT="$HERE/verdicts.json" ;;
esac
echo '{"type":"system","subtype":"init","session_id":"s"}'
printf '{"type":"result","subtype":"success","is_error":false,"result":"","structured_output":%s}\n' "$(cat "$OUT")"
"#;

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        claude: PathBuf,
    }

    impl Fixture {
        fn jobs(&self) -> Vec<String> {
            let log = self.claude.with_file_name("jobs.log");
            std::fs::read_to_string(log)
                .map(|text| text.lines().map(String::from).collect())
                .unwrap_or_default()
        }
    }

    fn written() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("lib");
        let library = Library::create(&root).unwrap();
        library
            .write_course(&Course {
                id: "lib".to_string(),
                title: "Rust ownership".to_string(),
                objectives: vec!["Explain moves".to_string()],
                chapters: vec![ChapterRef {
                    id: "moves".to_string(),
                    title: "Moves".to_string(),
                    summary: String::new(),
                    ..ChapterRef::default()
                }],
                ..Course::default()
            })
            .unwrap();
        let c = contract();
        std::fs::write(
            root.join("chapters/moves/index.md"),
            c["chapter"].as_str().unwrap(),
        )
        .unwrap();
        let claude = tmp.path().join("claude");
        std::fs::write(&claude, FAKE).unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(tmp.path().join("quiz.json"), c["agent_quiz"].to_string()).unwrap();
        std::fs::write(
            tmp.path().join("verdicts.json"),
            c["agent_verdicts"].to_string(),
        )
        .unwrap();
        Fixture {
            _tmp: tmp,
            root,
            claude,
        }
    }

    fn invoke(root: &Path, claude: &Path, cmd: &str, args: Value) -> Result<Value, Value> {
        let agents = crate::agents::tests::claude_agents(root, claude);
        let app = install(mock_builder(), ReaderState::open(root))
            .manage(GenerationState::new(root.to_path_buf(), agents))
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds");
        get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|body| body.deserialize::<Value>().expect("response is JSON"))
    }

    #[test]
    fn a_quiz_is_written_graded_and_remembered_through_ipc() {
        let fx = written();
        let c = contract();

        let quiz = invoke(
            &fx.root,
            &fx.claude,
            "generate_quiz",
            json!({"chapterId": "moves"}),
        )
        .unwrap();
        assert_eq!(quiz, c["generate_quiz"]);

        let answers = json!([{"question": quiz["questions"][0], "answer": c["learner_answer"]}]);
        let graded = invoke(
            &fx.root,
            &fx.claude,
            "grade_quiz",
            json!({"chapterId": "moves", "answers": answers}),
        )
        .unwrap();
        assert_eq!(graded, c["grade_quiz"]);

        let jobs = fx.jobs();
        assert_eq!(jobs.len(), 2);
        for argv in &jobs {
            assert!(argv.contains("[--permission-mode][plan]"), "{argv}");
            assert!(argv.contains("[--tools][]"), "no tool allowed: {argv}");
            assert!(!argv.contains("[--allowedTools]"), "{argv}");
        }

        let progress = invoke(&fx.root, &fx.claude, "course_progress", json!({})).unwrap();
        assert_eq!(progress[0]["objective"], "Explain moves");
        assert_eq!(progress[0]["attempts"], 1);
        assert_eq!(progress[0]["mastery"], 0.5, "one correct answer is capped");
        assert!(progress[0]["nextReviewAt"].is_number());
    }
}
