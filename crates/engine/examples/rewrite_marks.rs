//! One real chapter job and one real rewrite, through `ChapterJobs`.
//!
//! Claude, model `sonnet`, effort `low`. The rewrite is the second job. The
//! program refuses a second chapter job when the library already exists, and
//! refuses a second rewrite when `rewrite.done` is present.
//!
//! ```text
//! cargo run -p tutordocs-engine --example rewrite_marks -- "$HOME/tutordocs-ticket20-lib"
//! cargo run -p tutordocs-engine --example rewrite_marks -- "$HOME/tutordocs-ticket20-lib" continue
//! ```
//!
//! `continue` picks five sentences from the chapter already on disk and runs
//! only the rewrite. The caller deletes the library after recording the count.
//! The program prints field names and counts. It does not print a credential.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use tutordocs_agent::{
    AgentDriver, Capabilities, ClaudeDriver, Effort, EventStream, JobHandle, JobSpec, ModelId,
};
use tutordocs_engine::{ChapterJobs, Progress};
use tutordocs_store::{Annotation, ChapterRef, Course, Library, TextSelector};

const CHAPTER_ID: &str = "the-singing";
const SOURCE: &str = "\
# A kettle on the stove\n\
\n\
A kettle starts quiet. Cold water sits against the metal, and the heat from the burner has not yet reached the middle of the pot.\n\
\n\
The first sound is a light tick. Tiny bubbles form on the hot base, break free, and collapse when they meet water that is still cool. Each collapse is a small knock.\n\
\n\
The rumble comes next. Bubbles now survive the trip up through the water, and the column of water shakes as they rise. The lid chatters if it sits loose.\n\
\n\
Just before the boil, the noise often drops. The water near the top is hot enough that bubbles no longer collapse, so the knocks stop, and the kettle seems to hesitate.\n\
\n\
Then the whistle starts. Steam pushes through the narrow spout cap, and the fast air makes the tone people recognize from the next room. The heat is still arriving, but the water cannot get hotter than the boil while the lid is open to the air.\n\
\n\
Take the kettle off when the whistle is steady. Leaving it to scream boils water away and wastes the heat that already did its work.\n";

struct Tuned {
    inner: ClaudeDriver,
    model: ModelId,
}

#[tutordocs_agent::async_trait]
impl AgentDriver for Tuned {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    async fn start(&self, mut job: JobSpec) -> tutordocs_agent::anyhow::Result<JobHandle> {
        job.model = Some(self.model.clone());
        job.effort = Some(Effort::parse("low").expect("low is a valid effort token"));
        self.inner.start(job).await
    }

    async fn resume(
        &self,
        session: &str,
        mut job: JobSpec,
    ) -> tutordocs_agent::anyhow::Result<JobHandle> {
        job.model = Some(self.model.clone());
        job.effort = Some(Effort::parse("low").expect("low is a valid effort token"));
        self.inner.resume(session, job).await
    }

    fn events(&self, handle: &JobHandle) -> EventStream {
        self.inner.events(handle)
    }

    async fn cancel(&self, handle: &JobHandle) -> tutordocs_agent::anyhow::Result<()> {
        self.inner.cancel(handle).await
    }
}

fn course() -> Course {
    Course {
        id: "library".to_string(),
        title: "Why a kettle sings".to_string(),
        objectives: vec!["Explain the sounds a kettle makes as the water heats".to_string()],
        chapters: vec![ChapterRef {
            id: CHAPTER_ID.to_string(),
            title: "From a cold fill to a rolling boil".to_string(),
            summary: "What the quiet, the rumble and the whistle are, for a curious adult"
                .to_string(),
            ..ChapterRef::default()
        }],
        ..Course::default()
    }
}

fn report(phase: &'static str) -> impl Fn(Progress) + Send + Sync {
    move |progress| println!("progress {phase}: {progress:?}")
}

/// Five exact sentences, spread through the chapter. Each occurs once, is a
/// single line, and stays inside the basic multilingual plane so a UTF-16
/// `indexOf` and a Rust substring search agree.
fn pick_five(text: &str) -> Result<Vec<String>, String> {
    for min in [80_usize, 40, 20] {
        let mut found = Vec::new();
        for line in prose_lines(text) {
            for sentence in sentences_in(line) {
                if sentence.chars().count() < min {
                    continue;
                }
                if sentence.chars().any(|ch| (ch as u32) > 0xFFFF) {
                    continue;
                }
                if text.matches(sentence).count() != 1 {
                    continue;
                }
                if found.iter().any(|kept: &String| {
                    kept.contains(sentence) || sentence.contains(kept.as_str())
                }) {
                    continue;
                }
                found.push(sentence.to_string());
            }
        }
        println!("candidates min_chars={min} count={}", found.len());
        if found.len() >= 5 {
            return Ok(spread(&found, 5));
        }
    }
    Err("the chapter has fewer than 5 exact sentences to mark".into())
}

fn prose_lines(text: &str) -> Vec<&str> {
    let mut fenced = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || trimmed.starts_with('#') || trimmed.starts_with('|') {
            continue;
        }
        lines.push(line);
    }
    lines
}

fn sentences_in(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    for (n, &(idx, ch)) in chars.iter().enumerate() {
        if !matches!(ch, '.' | '!' | '?') {
            continue;
        }
        let end = idx + ch.len_utf8();
        let next = chars.get(n + 1).map(|&(_, next)| next);
        if next.is_some_and(|next| !next.is_whitespace()) {
            continue;
        }
        let sentence = line[start..end].trim();
        if !sentence.is_empty() {
            out.push(sentence);
        }
        start = end;
        while let Some(next) = line[start..].chars().next() {
            if !next.is_whitespace() {
                break;
            }
            start += next.len_utf8();
        }
    }
    out
}

fn spread(found: &[String], n: usize) -> Vec<String> {
    let last = found.len() - 1;
    (0..n).map(|i| found[i * last / (n - 1)].clone()).collect()
}

fn occurs(text: &str, exact: &str) -> bool {
    !exact.is_empty() && text.contains(exact)
}

fn string_fields(path: &Path, key: &str) -> Vec<String> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut found = Vec::new();
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        walk(&value, key, &mut found);
    }
    found.sort();
    found.dedup();
    found
}

fn walk(value: &serde_json::Value, key: &str, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (name, child) in map {
                if name == key {
                    if let Some(text) = child.as_str() {
                        out.push(text.to_string());
                    }
                }
                walk(child, key, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                walk(item, key, out);
            }
        }
        _ => {}
    }
}

fn prepare_library(library: &Path, fresh: bool) -> Result<(), Box<dyn Error>> {
    if fresh {
        if library.exists() {
            return Err(format!("{} already exists", library.display()).into());
        }
        let store = Library::create(library)?;
        store.write_course(&course())?;
        fs::write(library.join("sources/kettle.md"), SOURCE)?;
    } else if !library
        .join("chapters")
        .join(CHAPTER_ID)
        .join("index.md")
        .is_file()
    {
        return Err("continue needs the chapter file from the first job".into());
    }
    Ok(())
}

fn save_marks(library: &Path, quotes: &[String]) -> Result<(), Box<dyn Error>> {
    let store = Library::open(library)?;
    if !store
        .annotations_for(&format!("chapters/{CHAPTER_ID}"))?
        .is_empty()
    {
        return Err("marks are already stored; refusing a second set".into());
    }
    for (index, quote) in quotes.iter().enumerate() {
        let mut mark = Annotation::new(
            format!("mark-{}", index + 1),
            format!("chapters/{CHAPTER_ID}"),
        );
        mark.locator = (index + 1) as i64;
        mark.quote = quote.clone();
        mark.selectors = vec![TextSelector::TextQuoteSelector {
            exact: quote.clone(),
            prefix: String::new(),
            suffix: String::new(),
        }];
        store.put_annotation(&mark)?;
    }
    Ok(())
}

async fn run_phase(
    jobs: &ChapterJobs,
    library: &Path,
    binary: &Path,
    tap: &Path,
    regenerate: bool,
) -> Result<String, Box<dyn Error>> {
    let driver = Tuned {
        inner: ClaudeDriver::new(binary)?.with_ndjson_tap(tap),
        model: ModelId::parse("sonnet").expect("sonnet is a valid model id"),
    };
    let caps = driver.capabilities();
    if !caps.model || !caps.effort {
        return Err("the probed claude CLI does not advertise --model and --effort".into());
    }
    let started = Instant::now();
    let markdown = if regenerate {
        jobs.regenerate(library, &driver, CHAPTER_ID, None, &report("rewrite"))
            .await?
    } else {
        jobs.open(library, &driver, CHAPTER_ID, &report("chapter"))
            .await?
    };
    println!(
        "phase {} wall_ms {} bytes {}",
        if regenerate { "rewrite" } else { "chapter" },
        started.elapsed().as_millis(),
        markdown.len()
    );
    println!(
        "tap {} requestedModel {:?} effort {:?} apiKeySource {:?}",
        tap.display(),
        string_fields(tap, "requestedModel"),
        string_fields(tap, "effort"),
        string_fields(tap, "apiKeySource")
    );
    Ok(markdown)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let library = PathBuf::from(
        args.next()
            .ok_or("usage: rewrite_marks LIBRARY [continue]")?,
    );
    let continue_only = match args.next().as_deref() {
        None => false,
        Some("continue") => true,
        Some(other) => return Err(format!("unknown mode {other:?}").into()),
    };
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let home = PathBuf::from(std::env::var("HOME")?);
    if !library.starts_with(&home) {
        return Err("the library path must sit under HOME".into());
    }
    println!(
        "parent_anthropic_api_key_set: {}",
        std::env::var_os("ANTHROPIC_API_KEY").is_some()
    );

    let binary =
        PathBuf::from(std::env::var("CLAUDE_BINARY").unwrap_or_else(|_| "claude".to_string()));
    prepare_library(&library, !continue_only)?;
    let jobs = ChapterJobs::new();
    if !continue_only {
        let chapter = run_phase(
            &jobs,
            &library,
            &binary,
            &library.join("chapter-tap.ndjson"),
            false,
        )
        .await?;
        fs::write(library.join("chapter.done"), chapter.len().to_string())?;
        println!("chapter_done bytes {}", chapter.len());
    }

    let original = Library::open(&library)?
        .chapter_markdown(CHAPTER_ID)?
        .ok_or("the chapter file is missing")?;
    let quotes = pick_five(&original)?;
    for (index, quote) in quotes.iter().enumerate() {
        println!(
            "mark {} chars {} quote {}",
            index + 1,
            quote.chars().count(),
            quote
        );
        if !occurs(&original, quote) {
            return Err("a picked quote is not in the chapter".into());
        }
    }
    save_marks(&library, &quotes)?;
    if library.join("rewrite.done").exists() {
        return Err("rewrite.done exists; refusing a second rewrite".into());
    }
    let rewritten = run_phase(
        &jobs,
        &library,
        &binary,
        &library.join("rewrite-tap.ndjson"),
        true,
    )
    .await?;
    let mut kept = 0;
    for (index, quote) in quotes.iter().enumerate() {
        let hit = occurs(&rewritten, quote);
        if hit {
            kept += 1;
        }
        println!("verbatim {} {}", index + 1, if hit { "yes" } else { "no" });
    }
    println!("COUNTS {kept} of {}", quotes.len());
    fs::write(library.join("rewrite.done"), kept.to_string())?;
    Ok(())
}
