//! Quiz generation, free-text grading and the per-objective progress they
//! leave behind, driven through a fake agent that returns recorded replies.
//! No real agent job runs.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};
use tutordocs_agent::{
    AgentDriver, AgentEvent, AuthKind, Capabilities, EventStream, JobHandle, JobSpec,
};
use tutordocs_engine::{
    course_progress, generate_quiz, grade_answers, Answer, Outcome, Question, QuizError,
    GRADE_CONFIDENCE_THRESHOLD,
};
use tutordocs_store::{ChapterRef, Course, Library};

const CHAPTER: &str = "# Caching\n\
\n\
A write-back cache marks a changed line dirty instead of writing it to memory.\n\
\n\
A dirty line is written to memory only when it is evicted.\n\
\n\
Eviction picks the least recently used line.\n";

const OBJECTIVES: [&str; 2] = [
    "Explain when a write-back cache writes to memory",
    "Predict which line a cache evicts",
];

/// A fake agent that answers each job with the next recorded reply, in order.
struct Recorded {
    replies: Mutex<VecDeque<Value>>,
    jobs: Mutex<Vec<JobSpec>>,
}

impl Recorded {
    fn new(replies: impl IntoIterator<Item = Value>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            jobs: Mutex::new(Vec::new()),
        }
    }

    fn jobs(&self) -> Vec<JobSpec> {
        self.jobs.lock().unwrap().clone()
    }
}

#[tutordocs_agent::async_trait]
impl AgentDriver for Recorded {
    fn id(&self) -> &'static str {
        "recorded"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            headless: true,
            stream_json: true,
            structured_output: true,
            resume: true,
            system_append: true,
            tool_allowlist: true,
            read_only: true,
            model: false,
            effort: false,
            auth: AuthKind::SubscriptionOAuth,
        }
    }

    async fn start(&self, job: JobSpec) -> tutordocs_agent::anyhow::Result<JobHandle> {
        self.jobs.lock().unwrap().push(job);
        let value = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("a job ran that the test did not expect");
        Ok(JobHandle::scripted([
            AgentEvent::Session { id: "s".into() },
            AgentEvent::Result { value },
            AgentEvent::Done,
        ]))
    }

    async fn resume(&self, _: &str, job: JobSpec) -> tutordocs_agent::anyhow::Result<JobHandle> {
        self.start(job).await
    }

    fn events(&self, h: &JobHandle) -> EventStream {
        EventStream::for_job(h)
    }

    async fn cancel(&self, _: &JobHandle) -> tutordocs_agent::anyhow::Result<()> {
        Ok(())
    }
}

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

/// A library whose one chapter, `caching`, is written.
fn library() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("library");
    let library = Library::create(&root).unwrap();
    library
        .write_course(&Course {
            id: "systems".into(),
            title: "Systems".into(),
            objectives: OBJECTIVES.map(String::from).to_vec(),
            chapters: vec![
                ChapterRef {
                    id: "caching".into(),
                    title: "Caching".into(),
                    summary: String::new(),
                    ..ChapterRef::default()
                },
                ChapterRef {
                    id: "later".into(),
                    title: "Later".into(),
                    summary: String::new(),
                    ..ChapterRef::default()
                },
            ],
            ..Course::default()
        })
        .unwrap();
    std::fs::write(root.join("chapters/caching/index.md"), CHAPTER).unwrap();
    Env { _tmp: tmp, root }
}

/// A question that combines two facts from different paragraphs.
fn good_question(objective: u64) -> Value {
    json!({
        "objective": objective,
        "question": "You change a line and later it is the least recently used one. When does memory see your change?",
        "reference_answer": "When the line is evicted, because a dirty line is written only on eviction and eviction picks the least recently used line.",
        "combines": [
            "A dirty line is written to memory only when it is evicted.",
            "Eviction picks the least recently used line."
        ]
    })
}

async fn quiz(root: &Path) -> Vec<Question> {
    let agent = Recorded::new([json!({"questions": [good_question(1), good_question(2)]})]);
    generate_quiz(root, &agent, "caching")
        .await
        .unwrap()
        .questions
}

#[tokio::test]
async fn the_quiz_job_reads_the_chapter_from_its_prompt_and_may_use_no_tool() {
    let env = library();
    let agent = Recorded::new([json!({"questions": [good_question(1)]})]);

    let quiz = generate_quiz(&env.root, &agent, "caching").await.unwrap();

    assert_eq!(quiz.chapter_id, "caching");
    assert_eq!(quiz.questions.len(), 1);
    let q = &quiz.questions[0];
    assert_eq!(q.id, "q1");
    assert_eq!(q.objective, OBJECTIVES[0]);
    assert_eq!(q.combines.len(), 2);

    let jobs = agent.jobs();
    assert_eq!(jobs.len(), 1);
    let job = &jobs[0];
    assert!(job.timeout.is_none(), "a quiz job has no wall-clock limit");
    assert!(job.read_only, "the quiz job only reads");
    assert_eq!(
        job.allowed_tools,
        Some(Vec::new()),
        "the quiz job may use no tool"
    );
    assert!(job.json_schema.is_some());
    assert!(job
        .prompt
        .contains("Eviction picks the least recently used line."));
    assert!(job.prompt.contains(OBJECTIVES[1]));
    assert!(
        job.prompt.contains("never states together"),
        "the PV4 rule is in the prompt"
    );
}

#[tokio::test]
async fn a_question_that_does_not_combine_separate_facts_is_dropped() {
    let env = library();
    let one_paragraph = json!({
        "objective": 1,
        "question": "What does a write-back cache do with a changed line?",
        "reference_answer": "Marks it dirty.",
        "combines": [
            "A write-back cache marks a changed line dirty",
            "instead of writing it to memory"
        ]
    });
    let invented = json!({
        "objective": 2,
        "question": "Q",
        "reference_answer": "A",
        "combines": ["Caches are made of SRAM.", "Eviction picks the least recently used line."]
    });
    let agent = Recorded::new([json!({"questions": [one_paragraph, invented, good_question(2)]})]);

    let quiz = generate_quiz(&env.root, &agent, "caching").await.unwrap();

    assert_eq!(
        quiz.questions.len(),
        1,
        "only the synthesis question is kept"
    );
    assert_eq!(quiz.questions[0].objective, OBJECTIVES[1]);
    assert_eq!(quiz.questions[0].id, "q1");
}

#[tokio::test]
async fn a_fact_inside_a_longer_word_is_not_a_chapter_quote() {
    let env = library();
    std::fs::write(
        env.root.join("chapters/caching/index.md"),
        "The cellular membrane is a bilayer.\n\nThe nucleus stores DNA.\n",
    )
    .unwrap();
    let question = json!({
        "objective": 1,
        "question": "How do the cell and the nucleus relate?",
        "reference_answer": "The cell contains the nucleus.",
        "combines": ["cell", "nucleus"]
    });
    let agent = Recorded::new([json!({"questions": [question]})]);

    let err = generate_quiz(&env.root, &agent, "caching")
        .await
        .unwrap_err();

    let QuizError::InvalidQuiz(reason) = err else {
        panic!("expected the substring fact to be rejected, got {err:?}");
    };
    assert!(
        reason.contains("cell"),
        "cell is not a quote of cellular: {reason}"
    );
}

#[tokio::test]
async fn a_quiz_with_no_usable_question_is_refused_with_the_reasons() {
    let env = library();
    let single_fact = json!({
        "objective": 1,
        "question": "Q",
        "reference_answer": "A",
        "combines": ["Eviction picks the least recently used line."]
    });
    let bad_objective = json!({
        "objective": 3,
        "question": "Q",
        "reference_answer": "A",
        "combines": [
            "A dirty line is written to memory only when it is evicted.",
            "Eviction picks the least recently used line."
        ]
    });
    let agent = Recorded::new([json!({"questions": [single_fact, bad_objective]})]);

    let err = generate_quiz(&env.root, &agent, "caching")
        .await
        .unwrap_err();

    let QuizError::InvalidQuiz(reason) = err else {
        panic!("expected InvalidQuiz, got {err:?}");
    };
    assert!(reason.contains("two facts"), "{reason}");
    assert!(reason.contains("objective"), "{reason}");
}

#[tokio::test]
async fn an_unwritten_chapter_has_no_quiz_and_runs_no_job() {
    let env = library();
    let agent = Recorded::new([]);

    let err = generate_quiz(&env.root, &agent, "later").await.unwrap_err();

    assert!(matches!(err, QuizError::InvalidRequest(_)), "{err:?}");
    assert!(agent.jobs().is_empty());
}

fn verdict(id: &str, result: &str, confidence: f64) -> Value {
    json!({"question_id": id, "result": result, "confidence": confidence, "feedback": format!("{result} because of the eviction rule")})
}

#[tokio::test]
async fn the_threshold_is_point_eight_and_inclusive() {
    assert_eq!(GRADE_CONFIDENCE_THRESHOLD, 0.8);
    let env = library();
    let questions = quiz(&env.root).await;
    let answers = vec![
        Answer {
            question: questions[0].clone(),
            answer: "Only once it gets evicted, since that is the LRU line.".into(),
        },
        Answer {
            question: questions[1].clone(),
            answer: "The most recently used one.".into(),
        },
    ];
    let judge = Recorded::new([json!({"verdicts": [
        verdict("q1", "correct", 0.8),
        verdict("q2", "incorrect", 0.79),
    ]})]);

    let graded = grade_answers(&env.root, &judge, "caching", &answers, 1_000.0)
        .await
        .unwrap();

    assert_eq!(graded[0].result, Outcome::Correct);
    assert!(graded[0].is_final, "confidence 0.8 is final");
    assert_eq!(
        graded[1].result,
        Outcome::Ungraded,
        "below 0.8 is coerced to ungraded"
    );
    assert!(!graded[1].is_final);
    assert!(graded[1].feedback.contains("eviction rule"));

    let progress = course_progress(&env.root, 1_000.0).unwrap();
    assert_eq!(progress[0].objective, OBJECTIVES[0]);
    assert_eq!(progress[0].attempts, 1);
    // One correct answer is capped at 0.5.
    assert_eq!(progress[0].mastery, 0.5);
    assert_eq!(
        progress[1].attempts, 0,
        "a non-final verdict records nothing"
    );
    assert_eq!(progress[1].mastery, 0.0);
}

#[tokio::test]
async fn the_grading_job_checks_synthesis_with_no_tool_and_the_chapter_as_evidence() {
    let env = library();
    let questions = quiz(&env.root).await;
    let answers = vec![Answer {
        question: questions[0].clone(),
        answer: "When it is evicted.".into(),
    }];
    let judge = Recorded::new([json!({"verdicts": [verdict("q1", "partial", 0.9)]})]);

    grade_answers(&env.root, &judge, "caching", &answers, 1_000.0)
        .await
        .unwrap();

    let job = &judge.jobs()[0];
    assert!(
        job.timeout.is_none(),
        "a grading job has no wall-clock limit"
    );
    assert!(job.read_only);
    assert_eq!(job.allowed_tools, Some(Vec::new()));
    assert!(job.json_schema.is_some());
    assert!(job
        .prompt
        .contains("check negation and contradictions, not keyword overlap"));
    assert!(job.prompt.contains("When it is evicted."));
    assert!(job
        .prompt
        .contains("Eviction picks the least recently used line."));
}

#[tokio::test]
async fn blank_and_verbatim_answers_are_graded_without_a_job() {
    let env = library();
    let questions = quiz(&env.root).await;
    let answers = vec![
        Answer {
            question: questions[0].clone(),
            answer: "   ".into(),
        },
        Answer {
            question: questions[1].clone(),
            answer: format!("  {}  ", questions[1].reference_answer.to_uppercase()),
        },
    ];
    let judge = Recorded::new([]);

    let graded = grade_answers(&env.root, &judge, "caching", &answers, 1_000.0)
        .await
        .unwrap();

    assert!(judge.jobs().is_empty(), "no answer needed the agent");
    assert_eq!(graded[0].result, Outcome::Ungraded);
    assert!(!graded[0].is_final);
    assert_eq!(graded[1].result, Outcome::Correct);
    assert!(graded[1].is_final);
    let progress = course_progress(&env.root, 1_000.0).unwrap();
    assert_eq!(progress[0].attempts, 0);
    assert_eq!(progress[1].attempts, 1);
}

#[tokio::test]
async fn a_reply_that_does_not_match_the_answers_saves_no_grade() {
    let env = library();
    let questions = quiz(&env.root).await;
    let answers: Vec<Answer> = questions
        .iter()
        .map(|q| Answer {
            question: q.clone(),
            answer: "Some reasoning.".into(),
        })
        .collect();
    let replies = [
        json!({"verdicts": [verdict("q1", "correct", 1.0)]}),
        json!({"verdicts": [verdict("q1", "correct", 1.0), verdict("q9", "correct", 1.0)]}),
        json!({"verdicts": [verdict("q1", "correct", 1.0), verdict("q2", "excellent", 1.0)]}),
        json!({"verdicts": [verdict("q1", "correct", 1.0), verdict("q2", "correct", 1.5)]}),
    ];
    for reply in replies {
        let judge = Recorded::new([reply.clone()]);
        let err = grade_answers(&env.root, &judge, "caching", &answers, 1_000.0)
            .await
            .unwrap_err();
        assert!(
            matches!(err, QuizError::InvalidAssessment(_)),
            "{reply}: {err:?}"
        );
    }
    let progress = course_progress(&env.root, 1_000.0).unwrap();
    assert!(progress.iter().all(|p| p.attempts == 0));
}

#[tokio::test]
async fn progress_accumulates_per_objective_and_is_read_back_from_disk() {
    let env = library();
    let questions = quiz(&env.root).await;
    let first = &questions[0];
    for (at, result) in [
        (1_000.0, "incorrect"),
        (2_000.0, "partial"),
        (3_000.0, "correct"),
    ] {
        let judge = Recorded::new([json!({"verdicts": [verdict("q1", result, 0.95)]})]);
        let answers = vec![Answer {
            question: first.clone(),
            answer: "An attempt.".into(),
        }];
        grade_answers(&env.root, &judge, "caching", &answers, at)
            .await
            .unwrap();
    }

    let progress = course_progress(&env.root, 3_000.0).unwrap();

    assert_eq!(progress.len(), 2);
    assert_eq!(progress[0].attempts, 3);
    // Values 0, 0.5, 1 with weights 0.85, 0.95, 1.0: 1.475 / 2.8.
    assert!((progress[0].mastery - 1.475 / 2.8).abs() < 1e-9);
    assert!(progress[0].next_review_at.is_some());
    assert_eq!(progress[1].attempts, 0);
    assert_eq!(progress[1].next_review_at, None);
}

#[tokio::test]
async fn a_question_from_a_replaced_outline_is_not_graded() {
    let env = library();
    let mut stale = quiz(&env.root).await.remove(0);
    stale.objective = "An objective the outline no longer has".into();
    let judge = Recorded::new([]);

    let err = grade_answers(
        &env.root,
        &judge,
        "caching",
        &[Answer {
            question: stale,
            answer: "x".into(),
        }],
        1.0,
    )
    .await
    .unwrap_err();

    assert!(matches!(err, QuizError::InvalidRequest(_)), "{err:?}");
    assert!(judge.jobs().is_empty());
}
