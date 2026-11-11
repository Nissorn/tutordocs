//! A shelf is the library root: `courses/<course-id>/` plus a root
//! `.tutordocs/` that keeps app-wide settings (`agent.json`).
//!
//! Opening a root that still has a top-level `course.json` moves that course
//! into `courses/<slug>/` by rename. `store.db`, `store.db-wal` and
//! `store.db-shm` move in the same pass as the other `.tutordocs` entries.
//! Nothing is copied and nothing is deleted, so a crash between renames is
//! finished on the next open.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Result, StoreError};
use crate::library::{Course, Library};

const TAG_MAX_CHARS: usize = 40;
/// What a one-course root keeps in `.tutordocs/` that belongs to its course.
/// Anything else there, such as the agent settings and their temp file, is the
/// library's own and must never read as an unfinished move.
const COURSE_STORE_ENTRIES: &[&str] = &[
    "store.db",
    "store.db-wal",
    "store.db-shm",
    "store.db-journal",
    "staging",
    "course.json.tmp",
];

/// One course in the sidebar (`list_courses`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineCourseSummary {
    pub id: String,
    pub title: String,
    pub objectives: Vec<String>,
}

/// One course as the shelf lists it. `id` is the folder name under `courses/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseSummary {
    /// Folder name. It does not change after creation.
    pub id: String,
    /// The id inside `course.json`, which agent job records carry as their
    /// course id. Empty when that file cannot be read. After a migration it can
    /// differ from the folder name in `id`.
    pub stored_id: String,
    /// Title from `course.json`, empty when that file cannot be read.
    pub title: String,
    /// Tags from `meta.json`.
    pub tags: Vec<String>,
    /// Learning objectives from `course.json`.
    pub objectives: Vec<String>,
    /// Chapters named in the outline.
    pub chapter_count: u32,
    /// Each chapter's title in outline order, or its id when it has none.
    pub chapter_titles: Vec<String>,
    /// Chapters that already have markdown.
    pub written_chapter_count: u32,
    /// Unix seconds when the course folder was created, or 0 when unknown.
    pub created_at: u64,
    /// Unix seconds when the course was last marked opened.
    pub opened_at: Option<u64>,
    /// Why this course could not be read. The other courses still list.
    pub error: Option<String>,
    /// Whether the outline is a draft or approved.
    #[serde(default)]
    pub outline_status: crate::OutlineStatus,
    /// Whether an outline job for this course is running.
    #[serde(default)]
    pub planning: bool,
}

/// One course as recorded in trash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntry {
    pub trash_id: String,
    pub course_id: String,
    pub title: String,
    pub trashed_at: String,
    pub chapter_count: u32,
    pub written_chapter_count: u32,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CourseMeta {
    #[serde(default)]
    tags: Vec<String>,
    created_at: u64,
    #[serde(default)]
    opened_at: Option<u64>,
}

struct PendingMove {
    src: PathBuf,
    dest: PathBuf,
}

/// The open library root.
#[derive(Debug)]
pub struct Shelf {
    root: PathBuf,
}

impl Shelf {
    /// Open the shelf at `root`, creating it on first launch when the folder
    /// is missing or empty. A folder that already holds other files is never
    /// turned into a library.
    pub fn open_or_create(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        if is_missing_or_empty(root) {
            Self::create(root)
        } else {
            Self::open(root)
        }
    }

    /// Create the shelf layout in a missing or empty folder.
    pub fn create(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        if root.join("course.json").is_file() {
            return Err(StoreError::AlreadyExists(root.join("course.json")));
        }
        fs::create_dir_all(root.join("courses")).map_err(|source| StoreError::Io {
            path: root.join("courses"),
            source,
        })?;
        fs::create_dir_all(root.join(".tutordocs")).map_err(|source| StoreError::Io {
            path: root.join(".tutordocs"),
            source,
        })?;
        Ok(Self { root })
    }

    /// Open a library root, migrating a top-level course when one is there.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        if !root.exists() {
            return Err(StoreError::NotALibrary(root));
        }
        if root.join("course.json").is_file() || has_leftovers(&root)? {
            migrate(&root)?;
        }
        if !root.join("courses").is_dir() {
            return Err(StoreError::NotALibrary(root));
        }
        ensure_meta_for_courses(&root)?;
        Ok(Self { root })
    }

    /// The library root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every course folder, broken ones included.
    pub fn list(&self) -> Result<Vec<CourseSummary>> {
        let mut dirs = course_directories(&self.root)?;
        dirs.sort();
        Ok(dirs.iter().map(|dir| summary_of(dir)).collect())
    }

    /// Every readable course in the shelf, formatted as [`OutlineCourseSummary`].
    pub fn list_outline_courses(&self) -> Result<Vec<OutlineCourseSummary>> {
        let mut out = Vec::new();
        for summary in self.list()? {
            if summary.error.is_some() {
                continue;
            }
            let library = self.open_course(&summary.id)?;
            let course = library.course()?;
            out.push(OutlineCourseSummary {
                id: course.id,
                title: course.title,
                objectives: course.objectives,
            });
        }
        Ok(out)
    }

    /// Create `courses/<slug>/` from a title and optional tags.
    pub fn create_course(&self, title: &str, tags: &[String]) -> Result<CourseSummary> {
        let title = title.trim();
        if title.is_empty() {
            return Err(StoreError::Rejected("a course needs a title".into()));
        }
        let tags = normalize_tags(tags)?;
        let id = allocate_id(&self.root.join("courses"), &slug_from_title(title))?;
        let dir = self.root.join("courses").join(&id);
        {
            let library = Library::create(&dir)?;
            let mut course = library.course()?;
            course.title = title.to_string();
            library.write_course(&course)?;
        }
        write_meta(
            &dir,
            &CourseMeta {
                tags,
                created_at: unix_now(),
                opened_at: None,
            },
        )?;
        Ok(summary_of(&dir))
    }

    /// Replace a course's tags.
    pub fn set_tags(&self, course_id: &str, tags: &[String]) -> Result<CourseSummary> {
        let tags = normalize_tags(tags)?;
        let dir = self.course_dir(course_id)?;
        let mut meta = read_meta(&dir)?;
        if meta.created_at == 0 {
            meta.created_at = unix_now();
        }
        meta.tags = tags;
        write_meta(&dir, &meta)?;
        Ok(summary_of(&dir))
    }

    /// Record that the user opened this course.
    pub fn mark_opened(&self, course_id: &str) -> Result<CourseSummary> {
        let dir = self.course_dir(course_id)?;
        let mut meta = read_meta(&dir)?;
        if meta.created_at == 0 {
            meta.created_at = unix_now();
        }
        meta.opened_at = Some(unix_now());
        write_meta(&dir, &meta)?;
        Ok(summary_of(&dir))
    }

    /// Rename a course, leaving the shelf folder id unchanged.
    pub fn rename_course(&self, course_id: &str, title: &str) -> Result<CourseSummary> {
        let title = title.trim();
        if title.is_empty() {
            return Err(StoreError::Rejected("a course needs a title".into()));
        }
        let dir = self.course_dir(course_id)?;
        {
            let library = Library::open(&dir)?;
            let mut course = library.course()?;
            course.title = title.to_string();
            library.write_course(&course)?;
        }
        Ok(summary_of(&dir))
    }

    /// Move a course folder to trash under `.tutordocs/trash/<trashId>/`.
    pub fn trash_course(&self, course_id: &str) -> Result<TrashEntry> {
        let src = self.course_dir(course_id)?;
        let folder_id = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| StoreError::InvalidCourseId(course_id.to_string()))?
            .to_string();

        let trash_dir = self.root.join(".tutordocs").join("trash");
        fs::create_dir_all(&trash_dir).map_err(|source| StoreError::Io {
            path: trash_dir.clone(),
            source,
        })?;

        let now_sec = unix_now();
        let trash_id = allocate_trash_id(&trash_dir, &folder_id, now_sec)?;
        let dest = trash_dir.join(&trash_id);

        let (title, chapter_count, written_chapter_count) = match read_outline(&src) {
            Ok((course, chapters)) => {
                let written = chapters.iter().filter(|c| !c.is_placeholder).count() as u32;
                (course.title, chapters.len() as u32, written)
            }
            Err(_) => (String::new(), 0, 0),
        };

        let bytes = dir_bytes(&src);
        let trashed_at = rfc3339_utc(SystemTime::now());

        let entry = TrashEntry {
            trash_id: trash_id.clone(),
            course_id: folder_id,
            title,
            trashed_at,
            chapter_count,
            written_chapter_count,
            bytes,
        };

        fs::rename(&src, &dest).map_err(|source| StoreError::Io {
            path: dest.clone(),
            source,
        })?;

        let manifest_path = dest.join("trash.json");
        let manifest_json = serde_json::to_string_pretty(&entry)?;
        fs::write(&manifest_path, manifest_json).map_err(|source| StoreError::Io {
            path: manifest_path,
            source,
        })?;

        Ok(entry)
    }

    /// List all courses currently in trash, newest first.
    pub fn list_trash(&self) -> Result<Vec<TrashEntry>> {
        let trash_dir = self.root.join(".tutordocs").join("trash");
        if !trash_dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        let read_dir = fs::read_dir(&trash_dir).map_err(|source| StoreError::Io {
            path: trash_dir.clone(),
            source,
        })?;

        for item in read_dir {
            let item = item.map_err(|source| StoreError::Io {
                path: trash_dir.clone(),
                source,
            })?;
            let path = item.path();
            if !path.is_dir() {
                continue;
            }
            let trash_id = item.file_name().to_string_lossy().into_owned();
            let manifest_path = path.join("trash.json");
            if manifest_path.is_file() {
                if let Ok(text) = fs::read_to_string(&manifest_path) {
                    if let Ok(entry) = serde_json::from_str::<TrashEntry>(&text) {
                        entries.push(entry);
                        continue;
                    }
                }
            }
            let course_id = trash_id
                .rsplit_once('-')
                .map(|(cid, _)| cid)
                .unwrap_or(&trash_id)
                .to_string();
            let (title, chapter_count, written_chapter_count) = match read_outline(&path) {
                Ok((course, chapters)) => {
                    let written = chapters.iter().filter(|c| !c.is_placeholder).count() as u32;
                    (course.title, chapters.len() as u32, written)
                }
                Err(_) => (String::new(), 0, 0),
            };
            let bytes = dir_bytes(&path);
            let trashed_at = rfc3339_utc(
                fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .unwrap_or(UNIX_EPOCH),
            );
            entries.push(TrashEntry {
                trash_id,
                course_id,
                title,
                trashed_at,
                chapter_count,
                written_chapter_count,
                bytes,
            });
        }

        entries.sort_by(|a, b| {
            b.trashed_at
                .cmp(&a.trashed_at)
                .then_with(|| b.trash_id.cmp(&a.trash_id))
        });
        Ok(entries)
    }

    /// Restore a trashed course folder back into `courses/<courseId>`.
    pub fn restore_course(&self, trash_id: &str) -> Result<CourseSummary> {
        if !is_plain_component(trash_id) {
            return Err(StoreError::Rejected(format!(
                "invalid trash id {trash_id:?}: expected one plain directory name"
            )));
        }
        let trash_dir = self.root.join(".tutordocs").join("trash");
        let src = trash_dir.join(trash_id);
        if !src.is_dir() {
            return Err(StoreError::Rejected(format!("no trash entry {trash_id:?}")));
        }

        let manifest_path = src.join("trash.json");
        let course_id = if manifest_path.is_file() {
            fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|t| serde_json::from_str::<TrashEntry>(&t).ok())
                .map(|e| e.course_id)
        } else {
            None
        };
        let course_id = course_id.unwrap_or_else(|| {
            if let Ok((course, _)) = read_outline(&src) {
                if !course.id.is_empty() {
                    return course.id;
                }
            }
            trash_id
                .rsplit_once('-')
                .map(|(cid, _)| cid)
                .unwrap_or(trash_id)
                .to_string()
        });

        if !is_plain_component(&course_id) {
            return Err(StoreError::Rejected(format!(
                "invalid course id {course_id:?} in trash manifest"
            )));
        }

        let dest = self.root.join("courses").join(&course_id);
        if dest.exists() {
            return Err(StoreError::Rejected(format!(
                "cannot restore course: courses/{course_id} already exists"
            )));
        }

        let _ = fs::remove_file(&manifest_path);

        fs::rename(&src, &dest).map_err(|source| StoreError::Io {
            path: dest.clone(),
            source,
        })?;

        Ok(summary_of(&dest))
    }

    /// Permanently remove a trashed course folder.
    pub fn empty_trash(&self, trash_id: &str) -> Result<()> {
        if !is_plain_component(trash_id) {
            return Err(StoreError::Rejected(format!(
                "invalid trash id {trash_id:?}: expected one plain directory name"
            )));
        }
        let trash_dir = self.root.join(".tutordocs").join("trash");
        let target = trash_dir.join(trash_id);
        if !target.is_dir() {
            return Err(StoreError::Rejected(format!("no trash entry {trash_id:?}")));
        }
        fs::remove_dir_all(&target).map_err(|source| StoreError::Io {
            path: target,
            source,
        })
    }

    /// The course folder for a folder name or a `course.json` id.
    pub fn course_dir(&self, course_id: &str) -> Result<PathBuf> {
        course_dir(&self.root, course_id)
    }

    /// Open the course folder as a [`Library`].
    pub fn open_course(&self, course_id: &str) -> Result<Library> {
        Library::open(self.course_dir(course_id)?)
    }

    /// The course commands without a course id should use.
    ///
    /// A course marked opened wins. Two marks in the same unix second are
    /// ordered by `meta.json`'s mtime, because `opened_at` only stores seconds
    /// and a retag must not beat a later open in a different second.
    pub fn active_course_dir(&self) -> Result<PathBuf> {
        let list = self.list()?;
        let mut best: Option<(u64, SystemTime, String)> = None;
        for course in &list {
            if course.error.is_some() {
                continue;
            }
            let Some(opened) = course.opened_at else {
                continue;
            };
            let dir = course_dir(&self.root, &course.id)?;
            let mtime = meta_mtime(&dir);
            let replace = match &best {
                None => true,
                Some((at, previous_mtime, _)) => {
                    opened > *at || (opened == *at && mtime > *previous_mtime)
                }
            };
            if replace {
                best = Some((opened, mtime, course.id.clone()));
            }
        }
        if let Some((_, _, id)) = best {
            return course_dir(&self.root, &id);
        }
        let usable: Vec<_> = list
            .iter()
            .filter(|course| course.error.is_none())
            .collect();
        if usable.len() == 1 {
            return course_dir(&self.root, &usable[0].id);
        }
        if usable.is_empty() {
            return Err(StoreError::Rejected("this library has no course".into()));
        }
        Err(StoreError::Rejected("open a course first".into()))
    }
}

fn course_dir(root: &Path, course_id: &str) -> Result<PathBuf> {
    if !is_plain_component(course_id) {
        return Err(StoreError::InvalidCourseId(course_id.to_string()));
    }
    let direct = root.join("courses").join(course_id);
    if direct.is_dir() {
        return Ok(direct);
    }
    for dir in course_directories(root)? {
        let path = dir.join("course.json");
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(course) = serde_json::from_str::<Course>(&text) else {
            continue;
        };
        if course.id == course_id {
            return Ok(dir);
        }
    }
    Err(StoreError::CourseNotFound(course_id.to_string()))
}

fn migrate(root: &Path) -> Result<()> {
    if root.join("course.json").is_file() {
        migrate_pending(root)?;
    }
    if has_leftovers(root)? {
        finish_leftovers(root)?;
    }
    Ok(())
}

fn migrate_pending(root: &Path) -> Result<()> {
    let course_path = root.join("course.json");
    let course = read_course_file(&course_path)?;
    let id = migration_folder_id(&course);
    if !is_plain_component(&id) {
        return Err(StoreError::InvalidCourseId(id));
    }
    let dest = root.join("courses").join(&id);
    if dest.join("course.json").is_file() {
        return Err(StoreError::MigrationConflict(format!(
            "this library has both a course at the top level and a course named {id:?}; nothing was moved"
        )));
    }
    let mut moves = Vec::new();
    queue_move(&mut moves, course_path, dest.join("course.json"))?;
    queue_move(&mut moves, root.join("sources"), dest.join("sources"))?;
    queue_move(&mut moves, root.join("chapters"), dest.join("chapters"))?;
    for entry in movable_entries(root)? {
        let name = entry.file_name();
        queue_move(&mut moves, entry.path(), dest.join(".tutordocs").join(name))?;
    }
    apply_moves(&moves)?;
    Ok(())
}

fn finish_leftovers(root: &Path) -> Result<()> {
    let courses = course_directories(root)?;
    if courses.len() != 1 {
        return Err(StoreError::MigrationConflict(
            "cannot finish moving this library: more than one course is already here".into(),
        ));
    }
    let dest = &courses[0];
    let mut moves = Vec::new();
    queue_move(&mut moves, root.join("sources"), dest.join("sources"))?;
    queue_move(&mut moves, root.join("chapters"), dest.join("chapters"))?;
    for entry in movable_entries(root)? {
        let name = entry.file_name();
        queue_move(&mut moves, entry.path(), dest.join(".tutordocs").join(name))?;
    }
    apply_moves(&moves)?;
    Ok(())
}

fn queue_move(moves: &mut Vec<PendingMove>, src: PathBuf, dest: PathBuf) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    if dest.exists() {
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        return Err(StoreError::MigrationConflict(format!(
            "cannot finish moving this library: {name} is already in the course and still at the top level; nothing was merged"
        )));
    }
    moves.push(PendingMove { src, dest });
    Ok(())
}

fn apply_moves(moves: &[PendingMove]) -> Result<()> {
    for pending in moves {
        if let Some(parent) = pending.dest.parent() {
            fs::create_dir_all(parent).map_err(|source| StoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    for pending in moves {
        fs::rename(&pending.src, &pending.dest).map_err(|source| StoreError::Io {
            path: pending.dest.clone(),
            source,
        })?;
    }
    Ok(())
}

fn has_leftovers(root: &Path) -> Result<bool> {
    if root.join("sources").exists() || root.join("chapters").exists() {
        return Ok(true);
    }
    Ok(!movable_entries(root)?.is_empty())
}

fn movable_entries(root: &Path) -> Result<Vec<fs::DirEntry>> {
    let dir = root.join(".tutordocs");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|source| StoreError::Io {
        path: dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| StoreError::Io {
            path: dir.clone(),
            source,
        })?;
        if COURSE_STORE_ENTRIES
            .iter()
            .any(|name| entry.file_name() == *name)
        {
            out.push(entry);
        }
    }
    Ok(out)
}

fn ensure_meta_for_courses(root: &Path) -> Result<()> {
    for dir in course_directories(root)? {
        if dir.join("course.json").is_file() {
            ensure_meta(&dir)?;
        }
    }
    Ok(())
}

fn ensure_meta(dir: &Path) -> Result<()> {
    if meta_path(dir).is_file() {
        return Ok(());
    }
    write_meta(
        dir,
        &CourseMeta {
            tags: Vec::new(),
            created_at: unix_now(),
            opened_at: None,
        },
    )
}

fn course_directories(root: &Path) -> Result<Vec<PathBuf>> {
    let dir = root.join("courses");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|source| StoreError::Io {
        path: dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| StoreError::Io {
            path: dir.clone(),
            source,
        })?;
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            out.push(entry.path());
        }
    }
    Ok(out)
}

fn summary_of(dir: &Path) -> CourseSummary {
    let id = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (tags, created_at, opened_at, meta_error) = match read_meta(dir) {
        Ok(meta) => (meta.tags, meta.created_at, meta.opened_at, None),
        Err(err) => (Vec::new(), 0, None, Some(err.to_string())),
    };
    match read_outline(dir) {
        Ok((course, chapters)) => {
            let written = chapters
                .iter()
                .filter(|chapter| !chapter.is_placeholder)
                .count() as u32;
            CourseSummary {
                id,
                stored_id: course.id,
                title: course.title,
                tags,
                objectives: course.objectives,
                chapter_count: chapters.len() as u32,
                chapter_titles: chapters
                    .iter()
                    .map(|chapter| match chapter.title.trim() {
                        "" => chapter.id.clone(),
                        title => title.to_string(),
                    })
                    .collect(),
                written_chapter_count: written,
                created_at,
                opened_at,
                error: meta_error,
                outline_status: course.outline_status,
                planning: false,
            }
        }
        Err(err) => CourseSummary {
            id,
            stored_id: String::new(),
            title: String::new(),
            tags,
            objectives: Vec::new(),
            chapter_count: 0,
            chapter_titles: Vec::new(),
            written_chapter_count: 0,
            created_at,
            opened_at,
            error: Some(err.to_string()),
            outline_status: crate::OutlineStatus::Approved,
            planning: false,
        },
    }
}

fn read_outline(dir: &Path) -> Result<(Course, Vec<crate::Chapter>)> {
    let library = Library::open(dir)?;
    let course = library.course()?;
    let chapters = library.chapters()?;
    Ok((course, chapters))
}

fn read_course_file(path: &Path) -> Result<Course> {
    let text = fs::read_to_string(path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|err| StoreError::InvalidCourseJson(err.to_string()))
}

fn migration_folder_id(course: &Course) -> String {
    if !course.title.trim().is_empty() {
        return slug_from_title(&course.title);
    }
    if is_slug_id(&course.id) {
        return course.id.clone();
    }
    "course".to_string()
}

fn slug_from_title(title: &str) -> String {
    let mut out = String::new();
    let mut hyphen = false;
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            hyphen = false;
        } else if !out.is_empty() && !hyphen {
            out.push('-');
            hyphen = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "course".to_string()
    } else {
        out
    }
}

fn is_slug_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn allocate_trash_id(trash: &Path, course_id: &str, unix_sec: u64) -> Result<String> {
    let base = format!("{course_id}-{unix_sec}");
    if !trash.join(&base).exists() {
        return Ok(base);
    }
    for n in 1..10_000 {
        let candidate = format!("{base}-{n}");
        if !trash.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err(StoreError::Rejected(format!(
        "too many trash entries are already named {base}"
    )))
}

fn dir_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(meta) = fs::symlink_metadata(&path) {
                if meta.is_dir() {
                    total += dir_bytes(&path);
                } else {
                    total += meta.len();
                }
            }
        }
    }
    total
}

pub fn rfc3339_utc(t: SystemTime) -> String {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs() as i64,
        Err(before) => -(before.duration().as_secs() as i64),
    };
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let in_day = secs.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        in_day / 3600,
        in_day % 3600 / 60,
        in_day % 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

fn allocate_id(courses: &Path, base: &str) -> Result<String> {
    if !courses.join(base).exists() {
        return Ok(base.to_string());
    }
    for n in 2..10_000 {
        let candidate = format!("{base}-{n}");
        if !courses.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err(StoreError::Rejected(format!(
        "too many courses are already named {base}"
    )))
}

fn normalize_tags(tags: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            return Err(StoreError::Rejected(format!(
                "invalid tag {tag:?}: a tag cannot be empty"
            )));
        }
        if trimmed.chars().count() > TAG_MAX_CHARS {
            return Err(StoreError::Rejected(format!(
                "invalid tag {tag:?}: a tag is at most {TAG_MAX_CHARS} characters"
            )));
        }
        if seen.insert(trimmed.to_lowercase()) {
            out.push(trimmed.to_string());
        }
    }
    Ok(out)
}

fn meta_path(dir: &Path) -> PathBuf {
    dir.join(".tutordocs").join("meta.json")
}

fn read_meta(dir: &Path) -> Result<CourseMeta> {
    let path = meta_path(dir);
    if !path.is_file() {
        return Ok(CourseMeta {
            tags: Vec::new(),
            created_at: 0,
            opened_at: None,
        });
    }
    let text = fs::read_to_string(&path).map_err(|source| StoreError::Io {
        path: path.clone(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|err| StoreError::Rejected(format!("meta.json: {err}")))
}

fn write_meta(dir: &Path, meta: &CourseMeta) -> Result<()> {
    let path = meta_path(dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| StoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(meta)?;
    fs::write(&tmp, json).map_err(|source| StoreError::Io {
        path: tmp.clone(),
        source,
    })?;
    fs::rename(&tmp, &path).map_err(|source| StoreError::Io { path, source })
}

fn meta_mtime(dir: &Path) -> SystemTime {
    fs::metadata(meta_path(dir))
        .and_then(|meta| meta.modified())
        .unwrap_or(UNIX_EPOCH)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn is_plain_component(id: &str) -> bool {
    let mut parts = Path::new(id).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    )
}

fn is_missing_or_empty(root: &Path) -> bool {
    match fs::read_dir(root) {
        Ok(mut entries) => entries.next().is_none(),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// Where the library lives: `TUTORDOCS_LIBRARY`, else `~/TutorDocs`.
pub fn default_library_root() -> PathBuf {
    if let Some(path) = std::env::var_os("TUTORDOCS_LIBRARY") {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("TutorDocs")
}

/// The course folder a job or command should use. `course_id` is the folder
/// name or the id stored in `course.json`. `None` uses the opened course, or
/// the only one.
pub fn resolve_course_dir(
    root: &Path,
    course_id: Option<&str>,
) -> std::result::Result<PathBuf, String> {
    let shelf = Shelf::open_or_create(root).map_err(|e| e.to_string())?;
    match course_id {
        Some(id) => shelf.course_dir(id).map_err(|e| e.to_string()),
        None => shelf.active_course_dir().map_err(|e| e.to_string()),
    }
}
