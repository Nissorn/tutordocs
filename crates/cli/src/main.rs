//! Headless TutorDocs. The desktop app stays `tutordocs`. This binary is
//! `tutordocs-cli`. Every command calls the same store and engine functions
//! the Tauri commands call. Stdout is one JSON value. A job prints
//! `agent-job-line` payloads to stderr as they arrive.

use std::io::{stdout, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tutordocs_engine::{
    chapter_context, job_scope, list_answers, AgentOverrides, AgentsState, ApproveGenerate,
    GenerationState, JobRegistry,
};
use tutordocs_store::{
    default_library_root, list_course_templates, resolve_course_dir, CourseSetup, Library, Outline,
    Shelf,
};

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let code = err.exit_code();
            let _ = err.print();
            return exit_code(code);
        }
    };
    let outcome = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(dispatch(cli));
    let code = match outcome {
        Ok(()) => 0,
        Err(Fail::Usage(message)) => {
            eprintln!("{message}");
            let _ = std::io::stderr().flush();
            2
        }
        Err(Fail::Refuse(message)) => {
            write_json(&serde_json::json!({ "error": message }));
            1
        }
        Err(Fail::Interrupt(message)) => {
            write_json(&serde_json::json!({ "error": message }));
            130
        }
    };
    exit_code(code)
}

fn exit_code(code: i32) -> ExitCode {
    let _ = stdout().flush();
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

enum Fail {
    Usage(String),
    Refuse(String),
    Interrupt(String),
}

fn refused(err: impl ToString) -> Fail {
    Fail::Refuse(err.to_string())
}

fn write_json(value: &impl serde::Serialize) {
    if let Ok(text) = serde_json::to_string(value) {
        println!("{text}");
    }
    let _ = stdout().flush();
}

fn emit(value: &impl serde::Serialize) -> Result<(), Fail> {
    let text = serde_json::to_string(value).map_err(refused)?;
    println!("{text}");
    stdout().flush().map_err(refused)?;
    Ok(())
}

#[derive(Parser)]
#[command(
    name = "tutordocs-cli",
    about = "Run TutorDocs course commands without the desktop app",
    disable_help_subcommand = true
)]
struct Cli {
    /// Library root. Overrides TUTORDOCS_LIBRARY. Defaults to ~/TutorDocs.
    #[arg(long, global = true)]
    library: Option<PathBuf>,

    /// Course id from `courses`.
    #[arg(long, global = true)]
    course: Option<String>,

    /// Agent id for this process. Does not change the saved picker choice.
    #[arg(long, global = true)]
    agent: Option<String>,

    /// Model for this process. Does not change the saved picker choice.
    #[arg(long, global = true)]
    model: Option<String>,

    /// Effort for this process. Does not change the saved picker choice.
    #[arg(long, global = true)]
    effort: Option<String>,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List the courses in the library.
    Courses,
    #[command(subcommand)]
    Course(CourseCmd),
    /// List the built-in course templates.
    Templates,
    /// Copy files into the course's sources.
    Import {
        #[arg(required = true, num_args = 1.., value_name = "PATH")]
        paths: Vec<PathBuf>,
    },
    /// List the course's sources.
    Sources,
    #[command(subcommand)]
    Outline(OutlineCmd),
    /// Show this process's chapter queue for the course.
    Queue,
    #[command(subcommand)]
    Chapter(ChapterCmd),
    #[command(subcommand)]
    Section(SectionCmd),
    /// Ask a question about a chapter. The question is the last argument.
    Ask {
        #[arg(long)]
        chapter: String,
        #[arg(long)]
        quote: Option<String>,
        question: String,
    },
    /// List a chapter's answers, newest first.
    Answers {
        #[arg(long)]
        chapter: String,
    },
    /// A chapter's objectives, summary and sources.
    Context {
        #[arg(long)]
        chapter: String,
    },
}

#[derive(Subcommand)]
enum CourseCmd {
    /// Create a course. The title is the one positional argument.
    Create {
        title: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    #[command(subcommand)]
    Setup(SetupCmd),
    /// Move a course folder to trash.
    Trash { course: Option<String> },
    /// List all courses currently in trash.
    #[command(name = "trash-list")]
    TrashList,
    /// Restore a course from trash.
    Restore { trash_id: String },
    /// Permanently remove a course from trash.
    #[command(name = "trash-empty")]
    TrashEmpty { trash_id: String },
    /// Rename a course.
    Rename {
        arg1: String,
        arg2: Option<String>,
        #[arg(long)]
        course: Option<String>,
    },
}

#[derive(Subcommand)]
enum SetupCmd {
    Get,
    /// Set the setup from flags, or from one `--file` of setup JSON.
    Set {
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        level: Option<String>,
        #[arg(long)]
        hours: Option<u32>,
        #[arg(long)]
        language: Option<String>,
        #[arg(long)]
        guide: Option<String>,
    },
}

#[derive(Subcommand)]
enum OutlineCmd {
    Generate {
        #[arg(long)]
        topic: Option<String>,
        #[arg(long = "source")]
        sources: Vec<String>,
    },
    Show,
    /// Replace the outline with the JSON in FILE.
    Save {
        file: PathBuf,
        #[arg(long)]
        variant: Option<String>,
    },
    Revise {
        #[arg(long)]
        variant: Option<String>,
    },
    /// Approve the outline. `--generate all` writes every unwritten chapter
    /// in this process. `--generate on_open` is the default.
    Approve {
        #[arg(long)]
        generate: Option<String>,
        #[arg(long)]
        variant: Option<String>,
    },
    /// List all outline variants for the course.
    Variants,
    /// Show a specific outline variant.
    #[command(name = "variant-show")]
    VariantShow {
        variant_id: Option<String>,
        #[arg(long)]
        variant: Option<String>,
    },
    /// Branch a new outline variant from an existing one.
    Branch {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        label: Option<String>,
        name: Option<String>,
    },
}

#[derive(Subcommand)]
enum ChapterCmd {
    List,
    Open {
        #[arg(long)]
        chapter: String,
    },
    Rewrite {
        #[arg(long)]
        chapter: String,
    },
}

#[derive(Subcommand)]
enum SectionCmd {
    Revise {
        #[arg(long)]
        chapter: String,
        #[arg(long)]
        quote: String,
        #[arg(long)]
        instruction: String,
    },
}

struct Ctx {
    shelf: Shelf,
    course: Option<String>,
    generation: GenerationState,
    jobs: JobRegistry,
}

impl Ctx {
    fn open(cli: &Cli) -> Result<Self, Fail> {
        let root = cli.library.clone().unwrap_or_else(default_library_root);
        let shelf = Shelf::open_or_create(&root).map_err(refused)?;
        let agents = AgentsState::installed_with_overrides(
            &root,
            AgentOverrides {
                driver: cli.agent.clone(),
                model: cli.model.clone(),
                effort: cli.effort.clone(),
            },
        );
        let generation = GenerationState::new(root.clone(), agents);
        let jobs = JobRegistry::new();
        // Stderr is block-buffered when piped. A line that is not flushed
        // never reaches a caller waiting on the first job line.
        jobs.attach_emit(Arc::new(|name, payload| {
            if name != "agent-job-line" {
                return;
            }
            let Ok(line) = serde_json::to_string(&payload) else {
                return;
            };
            let mut err = std::io::stderr().lock();
            let _ = writeln!(err, "{line}");
            let _ = err.flush();
        }));
        Ok(Self {
            shelf,
            course: cli.course.clone(),
            generation,
            jobs,
        })
    }

    fn course_id(&self) -> Result<&str, Fail> {
        match self
            .course
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            Some(id) => Ok(id),
            None => Err(Fail::Usage("pass --course with the course id".to_string())),
        }
    }

    fn library(&self) -> Result<Library, Fail> {
        let id = self.course_id()?;
        self.shelf.open_course(id).map_err(refused)
    }
}

async fn dispatch(cli: Cli) -> Result<(), Fail> {
    if matches!(cli.command, Cmd::Templates) {
        return emit(&list_course_templates());
    }
    let ctx = Ctx::open(&cli)?;
    match cli.command {
        Cmd::Templates => unreachable!("templates returns before the shelf opens"),
        Cmd::Courses => {
            let mut courses = ctx.shelf.list().map_err(refused)?;
            let running_jobs = ctx.jobs.list();
            for course in &mut courses {
                let is_planning = running_jobs.iter().any(|job| {
                    job.status == tutordocs_engine::JobStatus::Running
                        && job.kind == tutordocs_engine::JobKind::Outline
                        && (job.course_id == course.id
                            || (!course.stored_id.is_empty() && job.course_id == course.stored_id))
                });
                if is_planning {
                    course.planning = true;
                }
            }
            emit(&courses)?;
        }
        Cmd::Course(CourseCmd::Create { title, tags }) => {
            emit(&ctx.shelf.create_course(&title, &tags).map_err(refused)?)?;
        }
        Cmd::Course(CourseCmd::Trash { course }) => {
            let id = match course.as_deref() {
                Some(id) => id,
                None => ctx.course_id()?,
            };
            let running_jobs = ctx.jobs.list();
            let (folder_id, stored_id) = {
                let dir = ctx.shelf.course_dir(id).map_err(refused)?;
                let folder_id = dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(id)
                    .to_string();
                let stored_id = if let Ok(course) = ctx
                    .shelf
                    .open_course(&folder_id)
                    .and_then(|lib| lib.course())
                {
                    course.id
                } else {
                    String::new()
                };
                (folder_id, stored_id)
            };
            for job in &running_jobs {
                if job.status == tutordocs_engine::JobStatus::Running
                    && (job.course_id == folder_id
                        || (!stored_id.is_empty() && job.course_id == stored_id))
                {
                    return Err(refused(format!(
                        "cannot trash course {id}: an agent job is currently running"
                    )));
                }
            }
            if let Ok(queue) = ctx.generation.get_chapter_queue(&folder_id) {
                if queue.running.is_some() || !queue.pending.is_empty() {
                    return Err(refused(format!(
                        "cannot trash course {id}: chapter queue is active"
                    )));
                }
            }
            emit(&ctx.shelf.trash_course(id).map_err(refused)?)?;
        }
        Cmd::Course(CourseCmd::TrashList) => {
            emit(&ctx.shelf.list_trash().map_err(refused)?)?;
        }
        Cmd::Course(CourseCmd::Restore { trash_id }) => {
            emit(&ctx.shelf.restore_course(&trash_id).map_err(refused)?)?;
        }
        Cmd::Course(CourseCmd::TrashEmpty { trash_id }) => {
            ctx.shelf.empty_trash(&trash_id).map_err(refused)?;
            emit(&())?;
        }
        Cmd::Course(CourseCmd::Rename { arg1, arg2, course }) => {
            let (course_id, title) = match (&course, &arg2) {
                (Some(id), None) => (id.as_str(), arg1.as_str()),
                (None, Some(t)) => (arg1.as_str(), t.as_str()),
                (Some(id), Some(t)) => (id.as_str(), t.as_str()),
                (None, None) => match ctx.course_id() {
                    Ok(id) => (id, arg1.as_str()),
                    Err(_) => (arg1.as_str(), ""),
                },
            };
            emit(&ctx.shelf.rename_course(course_id, title).map_err(refused)?)?;
        }
        Cmd::Course(CourseCmd::Setup(SetupCmd::Get)) => {
            emit(&ctx.library()?.course_setup().map_err(refused)?)?;
        }
        Cmd::Course(CourseCmd::Setup(SetupCmd::Set {
            file,
            template,
            level,
            hours,
            language,
            guide,
        })) => {
            let setup = setup_value(file, template, level, hours, language, guide)?;
            emit(&ctx.library()?.set_course_setup(&setup).map_err(refused)?)?;
        }
        Cmd::Import { paths } => {
            let paths: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
            emit(
                &ctx.library()?
                    .import_file_strings(&paths)
                    .map_err(refused)?,
            )?;
        }
        Cmd::Sources => emit(&ctx.library()?.course_sources().map_err(refused)?)?,
        Cmd::Outline(OutlineCmd::Show) => emit(&ctx.library()?.outline().map_err(refused)?)?,
        Cmd::Outline(OutlineCmd::Variants) => {
            emit(&ctx.library()?.list_outline_variants().map_err(refused)?)?
        }
        Cmd::Outline(OutlineCmd::VariantShow {
            variant_id,
            variant,
        }) => {
            let vid = variant
                .or(variant_id)
                .ok_or_else(|| Fail::Usage("variant id is required".into()))?;
            emit(&ctx.library()?.get_outline_variant(&vid).map_err(refused)?)?
        }
        Cmd::Outline(OutlineCmd::Branch { from, label, name }) => {
            let lbl = label
                .or(name)
                .ok_or_else(|| Fail::Usage("variant label is required".into()))?;
            let from_id = from.as_deref().unwrap_or("current");
            emit(
                &ctx.library()?
                    .branch_outline(from_id, &lbl)
                    .map_err(refused)?,
            )?
        }
        Cmd::Outline(OutlineCmd::Save { file, variant }) => {
            let outline: Outline = read_json(&file, "outline.json")?;
            if let Some(vid) = variant {
                emit(
                    &ctx.library()?
                        .save_outline_variant(&vid, &outline)
                        .map_err(refused)?,
                )?;
            } else {
                emit(&ctx.library()?.save_outline(&outline).map_err(refused)?)?;
            }
        }
        Cmd::Outline(OutlineCmd::Generate { topic, sources }) => {
            let id = ctx.course_id()?.to_string();
            emit(
                &ctx.generation
                    .generate_outline(&ctx.jobs, &id, topic, Some(sources))
                    .await
                    .map_err(refused)?,
            )?;
        }
        Cmd::Outline(OutlineCmd::Revise { variant }) => {
            let id = ctx.course_id()?.to_string();
            emit(
                &ctx.generation
                    .revise_outline(&ctx.jobs, &id, variant)
                    .await
                    .map_err(refused)?,
            )?;
        }
        Cmd::Outline(OutlineCmd::Approve { generate, variant }) => {
            approve(&ctx, generate.as_deref(), variant.as_deref()).await?
        }
        Cmd::Queue => {
            let id = ctx.course_id()?.to_string();
            emit(&ctx.generation.get_chapter_queue(&id).map_err(refused)?)?;
        }
        Cmd::Chapter(ChapterCmd::List) => {
            emit(&ctx.library()?.list_chapter_summaries().map_err(refused)?)?;
        }
        Cmd::Chapter(ChapterCmd::Open { chapter }) => {
            let id = ctx.course_id()?.to_string();
            emit(
                &ctx.generation
                    .open_chapter(&ctx.jobs, &id, &chapter, &|_| {})
                    .await
                    .map_err(refused)?,
            )?;
        }
        Cmd::Chapter(ChapterCmd::Rewrite { chapter }) => {
            let id = ctx.course_id()?.to_string();
            emit(
                &ctx.generation
                    .regenerate_chapter(&ctx.jobs, &id, &chapter, None, &|_| {})
                    .await
                    .map_err(refused)?,
            )?;
        }
        Cmd::Section(SectionCmd::Revise {
            chapter,
            quote,
            instruction,
        }) => {
            let id = ctx.course_id()?.to_string();
            emit(
                &ctx.generation
                    .revise_section(&ctx.jobs, &id, &chapter, &quote, &instruction)
                    .await
                    .map_err(refused)?,
            )?;
        }
        Cmd::Ask {
            chapter,
            quote,
            question,
        } => {
            let id = ctx.course_id()?.to_string();
            emit(
                &ctx.generation
                    .ask_about(&ctx.jobs, &id, &chapter, quote.as_deref(), &question)
                    .await
                    .map_err(refused)?,
            )?;
        }
        Cmd::Answers { chapter } => {
            let root = ctx.library()?.root().to_path_buf();
            emit(&list_answers(&root, &chapter).map_err(refused)?)?;
        }
        Cmd::Context { chapter } => {
            let root = ctx.library()?.root().to_path_buf();
            emit(&chapter_context(&root, &chapter).map_err(refused)?)?;
        }
    }
    Ok(())
}

fn setup_value(
    file: Option<PathBuf>,
    template: Option<String>,
    level: Option<String>,
    hours: Option<u32>,
    language: Option<String>,
    guide: Option<String>,
) -> Result<CourseSetup, Fail> {
    let has_file = file.is_some();
    let has_flags = template.is_some()
        || level.is_some()
        || hours.is_some()
        || language.is_some()
        || guide.is_some();
    if has_file && has_flags {
        return Err(Fail::Usage(
            "pass either --file or the setup flags, not both".into(),
        ));
    }
    if !has_file && !has_flags {
        return Err(Fail::Usage(
            "pass --file setup.json, or --template, --level and --language".into(),
        ));
    }
    if let Some(path) = file {
        return read_json(&path, "setup.json");
    }
    let (Some(template), Some(level), Some(language)) = (template, level, language) else {
        return Err(Fail::Usage(
            "pass --template, --level and --language, or pass --file".into(),
        ));
    };
    Ok(CourseSetup {
        template,
        level,
        hours_per_week: hours,
        language,
        guide: guide.unwrap_or_default(),
    })
}

fn read_json<T: serde::de::DeserializeOwned>(
    path: &std::path::Path,
    label: &str,
) -> Result<T, Fail> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        Fail::Refuse(format!(
            "could not read {label} at {}: {err}",
            path.display()
        ))
    })?;
    serde_json::from_str(&text).map_err(|err| {
        Fail::Refuse(format!(
            "could not read {label} at {}: {err}",
            path.display()
        ))
    })
}

fn generate_mode(raw: Option<&str>) -> Result<ApproveGenerate, Fail> {
    match raw {
        None | Some("on_open") => Ok(ApproveGenerate::OnOpen),
        Some("all") => Ok(ApproveGenerate::All),
        Some(other) => Err(Fail::Usage(format!(
            "--generate {other} is not on_open or all"
        ))),
    }
}

async fn approve(ctx: &Ctx, generate: Option<&str>, variant: Option<&str>) -> Result<(), Fail> {
    let mode = generate_mode(generate)?;
    let course_id = ctx.course_id()?.to_string();
    let (_before, queued) = ctx
        .generation
        .approve_outline_task(&course_id, mode, variant)
        .await
        .map_err(refused)?;
    if let Some((task, driver)) = queued {
        let course_dir =
            resolve_course_dir(ctx.generation.root(), Some(&course_id)).map_err(refused)?;
        let chapters = ctx.generation.chapters().clone();
        let interrupted = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&interrupted);
        // Ctrl-C sets the queue's cancelled flag. The running chapter job
        // finishes, and ChapterQueueTask stops before the next one starts.
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                flag.store(true, Ordering::SeqCst);
                chapters.cancel_queue(&course_dir);
            }
        });
        job_scope(ctx.jobs.log(), task.run(driver.as_ref())).await;
        if interrupted.load(Ordering::SeqCst) {
            return Err(Fail::Interrupt(
                "interrupted: the chapter queue stopped after the running job".into(),
            ));
        }
    }
    // The outline returned before the queue still says the chapters are
    // unwritten. Read it again once the jobs have published.
    let course_dir =
        resolve_course_dir(ctx.generation.root(), Some(&course_id)).map_err(refused)?;
    let outline = Library::open(&course_dir)
        .and_then(|library| library.outline())
        .map_err(refused)?;
    emit(&outline)
}
