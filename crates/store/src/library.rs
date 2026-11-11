//! The library: a plain folder of markdown as content truth, with a SQLite
//! store for annotations and progress. Neither is derived from the other.
//!
//! Layout (PROPOSAL "The data shape"): `sources/`, `chapters/<id>/` and
//! `course.json`. The database lives in a hidden `.tutordocs/` directory so the
//! visible folder stays plain markdown the user and the coding agent can edit.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::{Result, StoreError};
use crate::setup::CourseSetup;
use crate::{annotation, answer, db, mastery, source};

/// Whether a course outline is still a draft or has been approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutlineStatus {
    Draft,
    #[default]
    Approved,
}

/// One chapter reference as recorded in `course.json` (outline order).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChapterRef {
    /// Stable chapter id, matching `chapters/<id>/`.
    pub id: String,
    /// Human title for the sidebar.
    #[serde(default)]
    pub title: String,
    /// One line on what the chapter covers, from the outline. The chapter job
    /// reads it; a hand-written `course.json` may leave it out.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Source file names under `sources/` this chapter draws on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    /// The user's note to the AI for revising the outline, empty when none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

/// The outline carried by `course.json`: identity, objectives and chapter order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Course {
    /// Stable course id.
    pub id: String,
    /// Course title.
    #[serde(default)]
    pub title: String,
    /// Learning objectives, in order.
    #[serde(default)]
    pub objectives: Vec<String>,
    /// Chapters in outline order; the array order is the reading order.
    #[serde(default)]
    pub chapters: Vec<ChapterRef>,
    /// Whether the outline is still a draft or has been approved. A missing
    /// status in an existing `course.json` reads as `Approved`.
    #[serde(default, rename = "outlineStatus")]
    pub outline_status: OutlineStatus,
    /// Slug of the current outline variant.
    #[serde(
        default,
        rename = "currentVariantId",
        skip_serializing_if = "Option::is_none"
    )]
    pub current_variant_id: Option<String>,
    /// User-visible label for the current outline variant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The variant it was branched from, null for the first.
    #[serde(default, rename = "parentId", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// ISO 8601 creation timestamp for the current variant.
    #[serde(default, rename = "createdAt", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// A named variant of a course outline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineVariant {
    pub id: String,
    pub label: String,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub objectives: Vec<String>,
    pub chapters: Vec<OutlineChapter>,
}

/// Metadata summary of an outline variant for listings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantMeta {
    pub id: String,
    pub label: String,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub chapter_count: usize,
    pub written_chapter_count: usize,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredVariant {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub objectives: Vec<String>,
    #[serde(default)]
    pub chapters: Vec<ChapterRef>,
}

/// One chapter in the reviewable outline (`get_outline` / `save_outline`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineChapter {
    /// `None` for a chapter the user added; the backend assigns an id on save.
    pub id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub comment: String,
    /// True when `chapters/<id>/index.md` (or chapter markdown) exists; read-only for the UI.
    #[serde(default)]
    pub written: bool,
}

/// The reviewable course outline (`get_outline` / `save_outline`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outline {
    #[serde(default)]
    pub status: OutlineStatus,
    pub title: String,
    #[serde(default)]
    pub objectives: Vec<String>,
    #[serde(default)]
    pub chapters: Vec<OutlineChapter>,
}

/// One chapter row in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterSummary {
    pub id: String,
    pub title: String,
    pub placeholder: bool,
}

/// What a chapter holds right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ChapterBody {
    Ready { markdown: String },
    Placeholder,
}

/// A chapter as the document column needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterDoc {
    pub id: String,
    pub title: String,
    #[serde(flatten)]
    pub body: ChapterBody,
}

/// A chapter as listed to the UI: its outline entry plus whether content exists.
#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    /// Stable chapter id.
    pub id: String,
    /// Human title.
    pub title: String,
    /// Absolute path to `chapters/<id>/`.
    pub path: PathBuf,
    /// True when the directory holds no markdown yet (lazy generation target).
    pub is_placeholder: bool,
}

/// An open library: a folder plus its SQLite connection.
///
/// Dropping the value closes the connection. Durability is proven by reopening
/// the same folder with [`Library::open`], not by reusing this handle.
pub struct Library {
    root: PathBuf,
    conn: Connection,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Library")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl Library {
    /// Create a new library folder, writing the layout and a starter
    /// `course.json`. Refuses to clobber an existing `course.json`.
    pub fn create(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let course_path = root.join("course.json");
        if course_path.exists() {
            return Err(StoreError::AlreadyExists(course_path));
        }
        for dir in ["sources", "chapters", ".tutordocs"] {
            let path = root.join(dir);
            fs::create_dir_all(&path).map_err(|source| StoreError::Io { path, source })?;
        }
        let starter = Course {
            id: root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "course".to_string()),
            title: String::new(),
            objectives: Vec::new(),
            chapters: Vec::new(),
            outline_status: OutlineStatus::Approved,
            current_variant_id: None,
            label: None,
            parent_id: None,
            created_at: None,
        };
        let json = serde_json::to_string_pretty(&starter)?;
        fs::write(&course_path, json).map_err(|source| StoreError::Io {
            path: course_path,
            source,
        })?;
        Self::open(&root)
    }

    /// Open an existing library, reading `course.json` without rewriting it or
    /// any markdown. The schema bootstrap is idempotent and touches no rows.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let course_path = root.join("course.json");
        if !course_path.is_file() {
            return Err(StoreError::NotALibrary(root));
        }
        let meta = root.join(".tutordocs");
        fs::create_dir_all(&meta).map_err(|source| StoreError::Io {
            path: meta.clone(),
            source,
        })?;
        // Parse once to validate the shape; later reads stay fresh from disk.
        let mut course = read_course(&course_path)?;
        if course.current_variant_id.is_none() && !course.chapters.is_empty() {
            course.current_variant_id = Some("current".to_string());
            course.label = Some("Current".to_string());
            course.parent_id = None;
            course.created_at = Some(crate::shelf::rfc3339_utc(std::time::SystemTime::now()));
            let mut obj = read_course_object(&course_path).unwrap_or_default();
            if let serde_json::Value::Object(course_map) = serde_json::to_value(&course)? {
                for (k, v) in course_map {
                    obj.insert(k, v);
                }
            }
            write_course_object(&root, &obj)?;
        }
        let conn = db::open_db(&meta.join("store.db"))?;
        Ok(Self { root, conn })
    }

    /// The library folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read the course fresh from `course.json`; the file is the truth.
    pub fn course(&self) -> Result<Course> {
        read_course(&self.root.join("course.json"))
    }

    /// Read the optional `setup` from `course.json`.
    pub fn course_setup(&self) -> Result<Option<CourseSetup>> {
        read_course_setup(&self.root.join("course.json"))
    }

    /// Alias for [`Self::course_setup`].
    pub fn setup(&self) -> Result<Option<CourseSetup>> {
        self.course_setup()
    }

    /// Validate and save `setup` into `course.json`, keeping the rest of the
    /// course file untouched.
    pub fn set_course_setup(&self, setup: &CourseSetup) -> Result<CourseSetup> {
        setup.validate()?;
        let path = self.root.join("course.json");
        let mut obj = read_course_object(&path)?;
        obj.insert("setup".to_string(), serde_json::to_value(setup)?);
        write_course_object(&self.root, &obj)?;
        Ok(setup.clone())
    }

    /// Alias for [`Self::set_course_setup`].
    pub fn set_setup(&self, setup: &CourseSetup) -> Result<CourseSetup> {
        self.set_course_setup(setup)
    }

    /// Read the reviewable outline with each chapter's on-disk `written` flag.
    pub fn outline(&self) -> Result<Outline> {
        let course = self.course()?;
        let mut chapters = Vec::with_capacity(course.chapters.len());
        for chapter in course.chapters {
            let written = self.is_chapter_written(&chapter.id)?;
            chapters.push(OutlineChapter {
                id: Some(chapter.id),
                title: chapter.title,
                summary: chapter.summary,
                sources: chapter.sources,
                comment: chapter.comment,
                written,
            });
        }
        Ok(Outline {
            status: course.outline_status,
            title: course.title,
            objectives: course.objectives,
            chapters,
        })
    }

    /// Save the user's edits to the outline: titles, summaries, objectives,
    /// order, added chapters, removed unwritten chapters and comments.
    pub fn save_outline(&self, outline: &Outline) -> Result<Outline> {
        let current = self.course()?;
        let existing_ids: HashSet<&str> = current.chapters.iter().map(|c| c.id.as_str()).collect();
        let mut kept_ids = HashSet::new();
        for chapter in &outline.chapters {
            if let Some(id) = &chapter.id {
                if !existing_ids.contains(id.as_str()) {
                    return Err(StoreError::Rejected(format!(
                        "unknown chapter id {id:?}: not in this course"
                    )));
                }
                if !kept_ids.insert(id.clone()) {
                    return Err(StoreError::Rejected(format!(
                        "duplicate chapter id {id:?} in outline"
                    )));
                }
            }
        }
        let all_written = self.all_written_chapter_ids()?;
        for written_id in &all_written {
            if !kept_ids.contains(written_id) {
                return Err(StoreError::Rejected(format!(
                    "cannot remove written chapter {written_id:?}"
                )));
            }
        }
        for chapter in &current.chapters {
            if !kept_ids.contains(&chapter.id) && self.is_chapter_written(&chapter.id)? {
                return Err(StoreError::Rejected(format!(
                    "cannot remove written chapter {:?}",
                    chapter.id
                )));
            }
        }
        let mut used_ids = kept_ids;
        let mut chapters = Vec::with_capacity(outline.chapters.len());
        for chapter in &outline.chapters {
            if chapter.title.trim().is_empty() {
                return Err(StoreError::Rejected("a chapter needs a title".into()));
            }
            let id = match &chapter.id {
                Some(id) => id.clone(),
                None => allocate_chapter_id(&chapter.title, &mut used_ids),
            };
            chapters.push(ChapterRef {
                id,
                title: chapter.title.clone(),
                summary: chapter.summary.clone(),
                sources: chapter.sources.clone(),
                comment: chapter.comment.clone(),
            });
        }
        for old in &current.chapters {
            if !used_ids.contains(&old.id) {
                let _ = fs::remove_dir(self.root.join("chapters").join(&old.id));
            }
        }
        let (current_variant_id, label, parent_id, created_at) = match current.current_variant_id {
            Some(vid) => (
                Some(vid),
                current.label,
                current.parent_id,
                current.created_at,
            ),
            None => (
                Some("current".to_string()),
                Some("Current".to_string()),
                None,
                Some(crate::shelf::rfc3339_utc(std::time::SystemTime::now())),
            ),
        };
        let updated = Course {
            id: current.id,
            title: outline.title.clone(),
            objectives: outline.objectives.clone(),
            chapters,
            outline_status: current.outline_status,
            current_variant_id,
            label,
            parent_id,
            created_at,
        };
        self.write_course(&updated)?;
        self.outline()
    }

    /// Set of all chapter directory names that currently contain written markdown.
    pub fn all_written_chapter_ids(&self) -> Result<HashSet<String>> {
        let mut written = HashSet::new();
        let chapters_dir = self.root.join("chapters");
        if chapters_dir.is_dir() {
            let read_dir = fs::read_dir(&chapters_dir).map_err(|source| StoreError::Io {
                path: chapters_dir.clone(),
                source,
            })?;
            for entry in read_dir {
                let entry = entry.map_err(|source| StoreError::Io {
                    path: chapters_dir.clone(),
                    source,
                })?;
                let path = entry.path();
                if path.is_dir() {
                    let id = entry.file_name().to_string_lossy().into_owned();
                    if self.is_chapter_written(&id)? {
                        written.insert(id);
                    }
                }
            }
        }
        Ok(written)
    }

    /// List all outline variants for this course, ordered by creation time.
    pub fn list_outline_variants(&self) -> Result<Vec<VariantMeta>> {
        let course = self.course()?;
        let mut variants = Vec::new();
        if let Some(current_id) = &course.current_variant_id {
            let written_count = course
                .chapters
                .iter()
                .filter(|c| self.is_chapter_written(&c.id).unwrap_or(false))
                .count();
            variants.push(VariantMeta {
                id: current_id.clone(),
                label: course
                    .label
                    .clone()
                    .unwrap_or_else(|| "Current".to_string()),
                parent_id: course.parent_id.clone(),
                created_at: course.created_at.clone().unwrap_or_default(),
                chapter_count: course.chapters.len(),
                written_chapter_count: written_count,
                current: true,
            });
        } else if !course.chapters.is_empty() {
            let written_count = course
                .chapters
                .iter()
                .filter(|c| self.is_chapter_written(&c.id).unwrap_or(false))
                .count();
            variants.push(VariantMeta {
                id: "current".to_string(),
                label: "Current".to_string(),
                parent_id: None,
                created_at: String::new(),
                chapter_count: course.chapters.len(),
                written_chapter_count: written_count,
                current: true,
            });
        }
        let outlines_dir = self.root.join("outlines");
        if outlines_dir.is_dir() {
            let read_dir = fs::read_dir(&outlines_dir).map_err(|source| StoreError::Io {
                path: outlines_dir.clone(),
                source,
            })?;
            for entry in read_dir {
                let entry = entry.map_err(|source| StoreError::Io {
                    path: outlines_dir.clone(),
                    source,
                })?;
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(text) = fs::read_to_string(&path) {
                        if let Ok(stored) = serde_json::from_str::<StoredVariant>(&text) {
                            let written_count = stored
                                .chapters
                                .iter()
                                .filter(|c| self.is_chapter_written(&c.id).unwrap_or(false))
                                .count();
                            variants.push(VariantMeta {
                                id: stored.id,
                                label: stored.label,
                                parent_id: stored.parent_id,
                                created_at: stored.created_at,
                                chapter_count: stored.chapters.len(),
                                written_chapter_count: written_count,
                                current: false,
                            });
                        }
                    }
                }
            }
        }
        variants.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(variants)
    }

    /// Read a specific outline variant by id.
    pub fn get_outline_variant(&self, variant_id: &str) -> Result<OutlineVariant> {
        check_variant_id(variant_id)?;
        let course = self.course()?;
        let curr_id = course.current_variant_id.as_deref().unwrap_or("current");
        if variant_id == curr_id || (course.current_variant_id.is_none() && variant_id == "current")
        {
            let mut chapters = Vec::with_capacity(course.chapters.len());
            for ch in course.chapters {
                let written = self.is_chapter_written(&ch.id)?;
                chapters.push(OutlineChapter {
                    id: Some(ch.id),
                    title: ch.title,
                    summary: ch.summary,
                    sources: ch.sources,
                    comment: ch.comment,
                    written,
                });
            }
            return Ok(OutlineVariant {
                id: curr_id.to_string(),
                label: course.label.unwrap_or_else(|| "Current".to_string()),
                parent_id: course.parent_id,
                created_at: course.created_at.unwrap_or_default(),
                objectives: course.objectives,
                chapters,
            });
        }
        let path = self
            .root
            .join("outlines")
            .join(format!("{variant_id}.json"));
        if !path.is_file() {
            return Err(StoreError::Rejected(format!(
                "no variant {variant_id:?} in this course"
            )));
        }
        let text = fs::read_to_string(&path).map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        let stored: StoredVariant = serde_json::from_str(&text)
            .map_err(|e| StoreError::InvalidCourseJson(e.to_string()))?;
        let mut chapters = Vec::with_capacity(stored.chapters.len());
        for ch in stored.chapters {
            let written = self.is_chapter_written(&ch.id)?;
            chapters.push(OutlineChapter {
                id: Some(ch.id),
                title: ch.title,
                summary: ch.summary,
                sources: ch.sources,
                comment: ch.comment,
                written,
            });
        }
        Ok(OutlineVariant {
            id: stored.id,
            label: stored.label,
            parent_id: stored.parent_id,
            created_at: stored.created_at,
            objectives: stored.objectives,
            chapters,
        })
    }

    /// Branch a new outline variant from an existing one with a user-visible label.
    pub fn branch_outline(&self, from_variant_id: &str, label: &str) -> Result<OutlineVariant> {
        let label = label.trim();
        if label.is_empty() {
            return Err(StoreError::Rejected("label cannot be empty".to_string()));
        }
        let from = self.get_outline_variant(from_variant_id)?;

        let all_written = self.all_written_chapter_ids()?;
        let from_ids: HashSet<&str> = from
            .chapters
            .iter()
            .filter_map(|c| c.id.as_deref())
            .collect();
        for written_id in &all_written {
            if !from_ids.contains(written_id.as_str()) {
                return Err(StoreError::Rejected(format!(
                    "cannot remove written chapter {written_id:?}"
                )));
            }
        }

        let outlines_dir = self.root.join("outlines");
        fs::create_dir_all(&outlines_dir).map_err(|source| StoreError::Io {
            path: outlines_dir.clone(),
            source,
        })?;

        let course = self.course()?;
        let current_id = course.current_variant_id.as_deref().unwrap_or("current");
        let base_slug = chapter_slug(label);
        let base_slug = if base_slug.is_empty() {
            "variant"
        } else {
            &base_slug
        };
        let mut new_id = base_slug.to_string();
        if new_id == current_id || outlines_dir.join(format!("{new_id}.json")).exists() {
            let mut n = 2;
            loop {
                let candidate = format!("{base_slug}-{n}");
                if candidate != current_id
                    && !outlines_dir.join(format!("{candidate}.json")).exists()
                {
                    new_id = candidate;
                    break;
                }
                n += 1;
            }
        }

        let chapters: Vec<ChapterRef> = from
            .chapters
            .into_iter()
            .map(|c| ChapterRef {
                id: c.id.unwrap_or_default(),
                title: c.title,
                summary: c.summary,
                sources: c.sources,
                comment: c.comment,
            })
            .collect();

        let stored = StoredVariant {
            id: new_id.clone(),
            label: label.to_string(),
            parent_id: Some(from.id),
            created_at: crate::shelf::rfc3339_utc(std::time::SystemTime::now()),
            objectives: from.objectives,
            chapters,
        };

        let target_path = outlines_dir.join(format!("{new_id}.json"));
        let tmp = self
            .root
            .join(".tutordocs")
            .join(format!("{new_id}.json.tmp"));
        let json = serde_json::to_string_pretty(&stored)?;
        fs::write(&tmp, json).map_err(|source| StoreError::Io {
            path: tmp.clone(),
            source,
        })?;
        fs::rename(&tmp, &target_path).map_err(|source| StoreError::Io {
            path: target_path,
            source,
        })?;

        self.get_outline_variant(&new_id)
    }

    /// Save an outline to a specific variant file (or current variant).
    pub fn save_outline_variant(&self, variant_id: &str, outline: &Outline) -> Result<Outline> {
        check_variant_id(variant_id)?;
        let current = self.course()?;
        let curr_id = current.current_variant_id.as_deref().unwrap_or("current");
        if variant_id == curr_id {
            return self.save_outline(outline);
        }
        let path = self
            .root
            .join("outlines")
            .join(format!("{variant_id}.json"));
        if !path.is_file() {
            return Err(StoreError::Rejected(format!(
                "no variant {variant_id:?} in this course"
            )));
        }
        let text = fs::read_to_string(&path).map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        let mut stored: StoredVariant = serde_json::from_str(&text)
            .map_err(|e| StoreError::InvalidCourseJson(e.to_string()))?;

        let mut kept_ids = HashSet::new();
        for chapter in &outline.chapters {
            if let Some(id) = &chapter.id {
                if !kept_ids.insert(id.clone()) {
                    return Err(StoreError::Rejected(format!(
                        "duplicate chapter id {id:?} in outline"
                    )));
                }
            }
        }
        let all_written = self.all_written_chapter_ids()?;
        for written_id in &all_written {
            if !kept_ids.contains(written_id) {
                return Err(StoreError::Rejected(format!(
                    "cannot remove written chapter {written_id:?}"
                )));
            }
        }
        let mut used_ids = kept_ids;
        let mut chapters = Vec::with_capacity(outline.chapters.len());
        for chapter in &outline.chapters {
            if chapter.title.trim().is_empty() {
                return Err(StoreError::Rejected("a chapter needs a title".into()));
            }
            let id = match &chapter.id {
                Some(id) => id.clone(),
                None => allocate_chapter_id(&chapter.title, &mut used_ids),
            };
            chapters.push(ChapterRef {
                id,
                title: chapter.title.clone(),
                summary: chapter.summary.clone(),
                sources: chapter.sources.clone(),
                comment: chapter.comment.clone(),
            });
        }
        stored.objectives = outline.objectives.clone();
        stored.chapters = chapters;

        let tmp = self
            .root
            .join(".tutordocs")
            .join(format!("{variant_id}.json.tmp"));
        let json = serde_json::to_string_pretty(&stored)?;
        fs::write(&tmp, json).map_err(|source| StoreError::Io {
            path: tmp.clone(),
            source,
        })?;
        fs::rename(&tmp, &path).map_err(|source| StoreError::Io { path, source })?;

        let mut outline_chapters = Vec::with_capacity(stored.chapters.len());
        for ch in stored.chapters {
            let written = self.is_chapter_written(&ch.id)?;
            outline_chapters.push(OutlineChapter {
                id: Some(ch.id),
                title: ch.title,
                summary: ch.summary,
                sources: ch.sources,
                comment: ch.comment,
                written,
            });
        }
        Ok(Outline {
            status: OutlineStatus::Draft,
            title: outline.title.clone(),
            objectives: stored.objectives,
            chapters: outline_chapters,
        })
    }

    /// Swap the current variant with another variant atomically.
    pub fn set_current_variant(&self, variant_id: &str) -> Result<OutlineVariant> {
        check_variant_id(variant_id)?;
        let current = self.course()?;
        let curr_id = current.current_variant_id.as_deref().unwrap_or("current");
        if variant_id == curr_id {
            return self.get_outline_variant(variant_id);
        }
        let chosen_path = self
            .root
            .join("outlines")
            .join(format!("{variant_id}.json"));
        if !chosen_path.is_file() {
            return Err(StoreError::Rejected(format!(
                "no variant {variant_id:?} in this course"
            )));
        }
        let text = fs::read_to_string(&chosen_path).map_err(|source| StoreError::Io {
            path: chosen_path.clone(),
            source,
        })?;
        let chosen: StoredVariant = serde_json::from_str(&text)
            .map_err(|e| StoreError::InvalidCourseJson(e.to_string()))?;

        let all_written = self.all_written_chapter_ids()?;
        let chosen_ids: HashSet<&str> = chosen.chapters.iter().map(|c| c.id.as_str()).collect();
        for written_id in &all_written {
            if !chosen_ids.contains(written_id.as_str()) {
                return Err(StoreError::Rejected(format!(
                    "cannot remove written chapter {written_id:?}"
                )));
            }
        }

        let outlines_dir = self.root.join("outlines");
        fs::create_dir_all(&outlines_dir).map_err(|source| StoreError::Io {
            path: outlines_dir.clone(),
            source,
        })?;

        // 1. Write outgoing variant to outlines/<curr_id>.json
        let outgoing = StoredVariant {
            id: curr_id.to_string(),
            label: current.label.unwrap_or_else(|| "Current".to_string()),
            parent_id: current.parent_id,
            created_at: current
                .created_at
                .unwrap_or_else(|| crate::shelf::rfc3339_utc(std::time::SystemTime::now())),
            objectives: current.objectives,
            chapters: current.chapters,
        };
        let outgoing_path = outlines_dir.join(format!("{curr_id}.json"));
        let outgoing_tmp = self
            .root
            .join(".tutordocs")
            .join(format!("{curr_id}.json.tmp"));
        let outgoing_json = serde_json::to_string_pretty(&outgoing)?;
        fs::write(&outgoing_tmp, outgoing_json).map_err(|source| StoreError::Io {
            path: outgoing_tmp.clone(),
            source,
        })?;
        fs::rename(&outgoing_tmp, &outgoing_path).map_err(|source| StoreError::Io {
            path: outgoing_path,
            source,
        })?;

        // 2. Write chosen variant into course.json
        let updated_course = Course {
            id: current.id,
            title: current.title,
            objectives: chosen.objectives,
            chapters: chosen.chapters,
            outline_status: OutlineStatus::Draft,
            current_variant_id: Some(chosen.id.clone()),
            label: Some(chosen.label),
            parent_id: chosen.parent_id,
            created_at: Some(chosen.created_at),
        };
        self.write_course(&updated_course)?;

        // 3. Remove chosen variant file
        let _ = fs::remove_file(&chosen_path);

        self.get_outline_variant(&chosen.id)
    }

    fn is_chapter_written(&self, id: &str) -> Result<bool> {
        check_chapter_id(id)?;
        let dir = self.root.join("chapters").join(id);
        if dir.join("index.md").is_file() {
            return Ok(true);
        }
        has_markdown(&dir)
    }

    /// Replace `course.json` with `course` and create `chapters/<id>/` for
    /// every chapter that has no directory yet. A chapter already written keeps
    /// its markdown. The file is written beside the old one and renamed over
    /// it, so a reader never sees half an outline.
    pub fn write_course(&self, course: &Course) -> Result<()> {
        let mut course = course.clone();
        if course.current_variant_id.is_none() && !course.chapters.is_empty() {
            course.current_variant_id = Some("current".to_string());
            course.label = Some("Current".to_string());
            course.parent_id = None;
            course.created_at = Some(crate::shelf::rfc3339_utc(std::time::SystemTime::now()));
        }
        for chapter in &course.chapters {
            check_chapter_id(&chapter.id)?;
        }
        for chapter in &course.chapters {
            let dir = self.root.join("chapters").join(&chapter.id);
            fs::create_dir_all(&dir).map_err(|source| StoreError::Io { path: dir, source })?;
        }
        let path = self.root.join("course.json");
        let mut obj = read_course_object(&path).unwrap_or_default();
        if let serde_json::Value::Object(course_map) = serde_json::to_value(&course)? {
            for (k, v) in course_map {
                obj.insert(k, v);
            }
        }
        write_course_object(&self.root, &obj)
    }

    /// A chapter's markdown: `index.md`, else the first top-level markdown
    /// file by name. `None` while the chapter is a placeholder.
    pub fn chapter_markdown(&self, id: &str) -> Result<Option<String>> {
        check_chapter_id(id)?;
        let dir = self.root.join("chapters").join(id);
        let index = dir.join("index.md");
        let path = if index.is_file() {
            index
        } else {
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(source) => return Err(StoreError::Io { path: dir, source }),
            };
            let mut candidates: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.is_file() && is_markdown(path))
                .collect();
            candidates.sort();
            match candidates.into_iter().next() {
                Some(path) => path,
                None => return Ok(None),
            }
        };
        fs::read_to_string(&path)
            .map(Some)
            .map_err(|source| StoreError::Io { path, source })
    }

    /// List the course this folder holds. A course folder carries one
    /// `course.json`. A shelf of many courses is [`crate::Shelf`].
    pub fn courses(&self) -> Result<Vec<Course>> {
        Ok(vec![self.course()?])
    }

    /// List chapters in `course.json` order, flagging placeholders by scanning
    /// each `chapters/<id>/` for markdown. A chapter with no markdown lists as a
    /// placeholder, which is what lazy generation (ticket 07) depends on.
    pub fn chapters(&self) -> Result<Vec<Chapter>> {
        let course = self.course()?;
        let mut out = Vec::new();
        for chapter in course.chapters {
            let path = self.root.join("chapters").join(&chapter.id);
            let is_placeholder = !has_markdown(&path)?;
            out.push(Chapter {
                id: chapter.id,
                title: chapter.title,
                path,
                is_placeholder,
            });
        }
        Ok(out)
    }

    /// List chapters in `course.json` order as [`ChapterSummary`] wire rows.
    pub fn list_chapter_summaries(&self) -> Result<Vec<ChapterSummary>> {
        let chapters = self.chapters()?;
        Ok(chapters
            .into_iter()
            .map(|chapter| ChapterSummary {
                id: chapter.id,
                title: chapter.title,
                placeholder: chapter.is_placeholder,
            })
            .collect())
    }

    /// Read one chapter as [`ChapterDoc`], refusing unknown chapters or path escapes.
    pub fn read_chapter_doc(
        &self,
        course_id: &str,
        chapter_id: &str,
    ) -> std::result::Result<ChapterDoc, String> {
        if !crate::shelf::is_plain_component(chapter_id) {
            return Err(format!("invalid chapter id: {chapter_id:?}"));
        }
        let chapters = self.chapters().map_err(|e| e.to_string())?;
        let chapter = chapters
            .into_iter()
            .find(|chapter| chapter.id == chapter_id)
            .ok_or_else(|| format!("no chapter {chapter_id:?} in course {course_id:?}"))?;
        let markdown = if chapter.is_placeholder {
            None
        } else {
            self.chapter_markdown(&chapter.id)
                .map_err(|e| e.to_string())?
        };
        Ok(ChapterDoc {
            id: chapter.id,
            title: chapter.title,
            body: match markdown {
                Some(markdown) => ChapterBody::Ready { markdown },
                None => ChapterBody::Placeholder,
            },
        })
    }

    /// List the markdown sources under `sources/` by name. A file the user
    /// dropped in and a capture the app wrote list identically; only captures
    /// carry an origin.
    pub fn sources(&self) -> Result<Vec<source::Source>> {
        source::list(&self.root)
    }

    /// List every visible course source under `sources/` with its kind, origin
    /// metadata and byte size.
    pub fn course_sources(&self) -> Result<Vec<source::CourseSource>> {
        source::list_all(&self.root)
    }

    /// Import files or folders into `sources/`, skipping unsupported types,
    /// duplicates and symlinks that leave a chosen folder.
    pub fn import_files<P: AsRef<Path>>(&self, paths: &[P]) -> Result<Vec<source::ImportResult>> {
        source::import_files(&self.root, paths)
    }

    /// Import string paths into `sources/`.
    pub fn import_file_strings(&self, paths: &[String]) -> Result<Vec<source::ImportResult>> {
        let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        self.import_files(&path_bufs)
    }

    /// Read one source's markdown by its listed name.
    pub fn read_source(&self, name: &str) -> Result<String> {
        source::read(&self.root, name)
    }

    /// The hidden folder where captures are staged before they join `sources/`.
    /// It sits inside the library so the final move never crosses a filesystem.
    pub fn staging_dir(&self) -> Result<PathBuf> {
        source::staging_dir(&self.root)
    }

    /// Move a staged markdown file into `sources/` as `name`, recording where
    /// it came from. Refuses to replace an existing source.
    pub fn commit_capture(
        &self,
        staged: &Path,
        name: &str,
        origin: &source::SourceOrigin,
    ) -> Result<source::Source> {
        source::commit_capture(&self.root, staged, name, origin)
    }

    /// Insert or replace an annotation.
    pub fn put_annotation(&self, a: &annotation::Annotation) -> Result<()> {
        annotation::put(&self.conn, a)
    }

    /// Read one annotation by id.
    pub fn annotation(&self, id: &str) -> Result<Option<annotation::Annotation>> {
        annotation::get(&self.conn, id)
    }

    /// List a material's annotations in locator order.
    pub fn annotations_for(&self, material_id: &str) -> Result<Vec<annotation::Annotation>> {
        annotation::list_for(&self.conn, material_id)
    }

    /// Explicitly delete a mark; returns whether a row was removed.
    pub fn delete_annotation(&self, id: &str) -> Result<bool> {
        annotation::delete(&self.conn, id)
    }

    /// Append one mastery event and return its monotone id.
    pub fn append_mastery_event(&self, e: &mastery::NewMasteryEvent) -> Result<i64> {
        mastery::append(&self.conn, e)
    }

    /// Append every event in one transaction. A failure leaves the log unchanged.
    pub fn append_mastery_events(&self, events: &[mastery::NewMasteryEvent]) -> Result<Vec<i64>> {
        let tx = self.conn.unchecked_transaction()?;
        let mut ids = Vec::with_capacity(events.len());
        for event in events {
            ids.push(mastery::append(&tx, event)?);
        }
        tx.commit()?;
        Ok(ids)
    }

    /// Read a path's events in append order.
    pub fn mastery_events(&self, path_id: &str) -> Result<Vec<mastery::MasteryEvent>> {
        mastery::list_for_path(&self.conn, path_id)
    }

    /// Derive the repetition state from the log. Nothing is stored; reopening
    /// the database and calling this again yields the identical state.
    pub fn repetition_state(&self, path_id: &str, now: f64) -> Result<mastery::RepetitionState> {
        let events = self.mastery_events(path_id)?;
        Ok(mastery::derive_repetition_state(&events, now))
    }

    /// Save one answer for `chapter_id` and return the stored record with its id.
    pub fn save_answer(
        &self,
        chapter_id: &str,
        entry: &answer::NewAnswer,
    ) -> Result<answer::ChapterAnswer> {
        check_chapter_id(chapter_id)?;
        answer::save(&self.conn, chapter_id, entry)
    }

    /// List `chapter_id`'s answers, newest first.
    pub fn list_answers(&self, chapter_id: &str) -> Result<Vec<answer::ChapterAnswer>> {
        check_chapter_id(chapter_id)?;
        answer::list_for(&self.conn, chapter_id)
    }
}

/// A chapter id names one directory under `chapters/`. Anything that could
/// climb out of it is refused before it touches a path.
fn check_chapter_id(id: &str) -> Result<()> {
    let mut parts = Path::new(id).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(_)), None) => Ok(()),
        _ => Err(StoreError::InvalidChapterId(id.to_string())),
    }
}

fn check_variant_id(id: &str) -> Result<()> {
    let mut parts = Path::new(id).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(_)), None) if !id.starts_with('.') => Ok(()),
        _ => Err(StoreError::Rejected(format!("invalid variant id {id:?}"))),
    }
}

#[derive(Deserialize)]
struct CourseSetupEnvelope {
    #[serde(default)]
    setup: Option<CourseSetup>,
}

fn read_course(path: &Path) -> Result<Course> {
    let text = fs::read_to_string(path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let course: Course =
        serde_json::from_str(&text).map_err(|e| StoreError::InvalidCourseJson(e.to_string()))?;
    let env: CourseSetupEnvelope =
        serde_json::from_str(&text).map_err(|e| StoreError::InvalidCourseJson(e.to_string()))?;
    if let Some(setup) = &env.setup {
        setup
            .validate()
            .map_err(|e| StoreError::InvalidCourseJson(e.to_string()))?;
    }
    Ok(course)
}

fn read_course_setup(path: &Path) -> Result<Option<CourseSetup>> {
    let text = fs::read_to_string(path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let env: CourseSetupEnvelope =
        serde_json::from_str(&text).map_err(|e| StoreError::InvalidCourseJson(e.to_string()))?;
    if let Some(setup) = &env.setup {
        setup.validate()?;
    }
    Ok(env.setup)
}

fn read_course_object(path: &Path) -> Result<serde_json::Map<String, serde_json::Value>> {
    let text = fs::read_to_string(path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let _: Course =
        serde_json::from_str(&text).map_err(|e| StoreError::InvalidCourseJson(e.to_string()))?;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        _ => Err(StoreError::InvalidCourseJson(
            "expected JSON object".to_string(),
        )),
    }
}

fn write_course_object(
    root: &Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    let path = root.join("course.json");
    let tmp = root.join(".tutordocs").join("course.json.tmp");
    let json = serde_json::to_string_pretty(obj)?;
    fs::write(&tmp, json).map_err(|source| StoreError::Io {
        path: tmp.clone(),
        source,
    })?;
    fs::rename(&tmp, &path).map_err(|source| StoreError::Io { path, source })
}

/// True when any markdown file exists under `dir` (recursive). A missing
/// directory is a placeholder, not an error.
fn has_markdown(dir: &Path) -> Result<bool> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(StoreError::Io {
                path: dir.to_path_buf(),
                source,
            })
        }
    };
    for entry in entries {
        let entry = entry.map_err(|source| StoreError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            if has_markdown(&path)? {
                return Ok(true);
            }
        } else if is_markdown(&path) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_markdown(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md") | Some("markdown")
    )
}

fn chapter_slug(title: &str) -> String {
    let mut out = String::new();
    let mut hyphen = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            hyphen = false;
        } else if !out.is_empty() && !hyphen {
            out.push('-');
            hyphen = true;
        }
        if out.len() >= 56 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "chapter".to_string()
    } else {
        out
    }
}

fn allocate_chapter_id(title: &str, used: &mut HashSet<String>) -> String {
    let base = chapter_slug(title);
    if used.insert(base.clone()) {
        return base;
    }
    for n in 2.. {
        let candidate = format!("{base}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}
