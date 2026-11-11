//! Sources: the markdown files under `sources/` that chapters cite.
//!
//! A source is a file the user dropped in or the agent captured from a URL.
//! Both list and read the same way, so chapter generation never special-cases
//! how a source arrived.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, StoreError};

const MAX_IMPORT_DEPTH: usize = 3;
const MAX_IMPORT_FILES: usize = 200;

/// Kind of a course source in `sources/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Pdf,
    Markdown,
    Text,
    Url,
    Other,
}

/// One source in `sources/` as returned by `list_course_sources` and `import_files`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseSource {
    pub name: String,
    pub path: String,
    pub kind: SourceKind,
    pub url: Option<String>,
    pub fetched_at: Option<String>,
    pub bytes: u64,
}

/// Outcome of importing one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportStatus {
    Imported,
    Skipped,
}

/// Result of importing one file or skipped entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub path: String,
    pub status: ImportStatus,
    pub reason: Option<String>,
    pub source: Option<CourseSource>,
}

/// Where a captured source came from. Present only for sources the app
/// captured; a file the user dropped in carries no origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceOrigin {
    /// The URL the user submitted, exactly as accepted.
    pub url: String,
    /// When the page was fetched, as an RFC 3339 UTC timestamp.
    pub fetched_at: String,
}

/// One markdown source as listed to the UI and to chapter generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// File name under `sources/`, the stable handle for [`read`].
    pub name: String,
    /// Absolute path of the markdown file.
    pub path: PathBuf,
    /// Where the source was captured from, when the app captured it.
    pub origin: Option<SourceOrigin>,
}

/// A captured or markdown source as the webview reads it from `ingest_url`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDto {
    pub name: String,
    pub path: String,
    pub url: Option<String>,
    pub fetched_at: Option<String>,
}

impl From<Source> for SourceDto {
    fn from(source: Source) -> Self {
        let (url, fetched_at) = match source.origin {
            Some(origin) => (Some(origin.url), Some(origin.fetched_at)),
            None => (None, None),
        };
        Self {
            name: source.name,
            path: source.path.to_string_lossy().into_owned(),
            url,
            fetched_at,
        }
    }
}

/// A capture's origin lives beside it as `<name>.source.json`.
const ORIGIN_SUFFIX: &str = ".source.json";

pub(crate) fn staging_dir(root: &Path) -> Result<PathBuf> {
    let dir = root.join(".tutordocs").join("staging");
    fs::create_dir_all(&dir).map_err(|source| io_err(&dir, source))?;
    Ok(dir)
}

pub(crate) fn commit_capture(
    root: &Path,
    staged: &Path,
    name: &str,
    origin: &SourceOrigin,
) -> Result<Source> {
    check_name(name)?;
    let dest = root.join("sources").join(name);
    if dest.exists() {
        return Err(StoreError::SourceExists(name.to_string()));
    }
    let origin_path = origin_path(&dest);

    // The origin goes in first and the markdown last: the markdown appearing is
    // the commit point, and an origin with no markdown is an invisible orphan.
    let tmp = staging_dir(root)?.join(format!("{name}{ORIGIN_SUFFIX}.tmp"));
    fs::write(&tmp, serde_json::to_vec_pretty(origin)?).map_err(|source| io_err(&tmp, source))?;
    fs::rename(&tmp, &origin_path).map_err(|source| io_err(&origin_path, source))?;

    // A hard link is an atomic rename that refuses to replace an existing file.
    fs::hard_link(staged, &dest).map_err(|source| io_err(&dest, source))?;
    let _ = fs::remove_file(staged);
    Ok(Source {
        name: name.to_string(),
        path: dest,
        origin: Some(origin.clone()),
    })
}

fn origin_path(source: &Path) -> PathBuf {
    let mut name = source.as_os_str().to_os_string();
    name.push(ORIGIN_SUFFIX);
    PathBuf::from(name)
}

fn read_origin(source: &Path) -> Option<SourceOrigin> {
    let bytes = fs::read(origin_path(source)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(crate) fn list(root: &Path) -> Result<Vec<Source>> {
    let dir = root.join("sources");
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|source| io_err(&dir, source))? {
        let entry = entry.map_err(|source| io_err(&dir, source))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if is_file && is_source_name(&name) {
            let path = entry.path();
            out.push(Source {
                origin: read_origin(&path),
                path,
                name,
            });
        }
    }
    // By stem first, so "a.md" precedes "a-2.md" ('-' sorts before '.').
    out.sort_by(|a, b| (stem_of(&a.name), &a.name).cmp(&(stem_of(&b.name), &b.name)));
    Ok(out)
}

pub(crate) fn read(root: &Path, name: &str) -> Result<String> {
    check_name(name)?;
    let path = root.join("sources").join(name);
    fs::read_to_string(&path).map_err(|source| match source.kind() {
        std::io::ErrorKind::NotFound => StoreError::SourceNotFound(name.to_string()),
        _ => io_err(&path, source),
    })
}

/// A source name is one plain component that `list` would itself show.
fn check_name(name: &str) -> Result<()> {
    let mut parts = Path::new(name).components();
    let one_plain_component = matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    );
    if one_plain_component && is_source_name(name) {
        Ok(())
    } else {
        Err(StoreError::InvalidSourceName(name.to_string()))
    }
}

fn stem_of(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn is_source_name(name: &str) -> bool {
    !name.starts_with('.')
        && matches!(
            Path::new(name).extension().and_then(|e| e.to_str()),
            Some("md") | Some("markdown")
        )
}

fn io_err(path: &Path, source: std::io::Error) -> StoreError {
    StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

impl ImportResult {
    fn skipped(path: &Path, reason: impl Into<String>) -> Self {
        Self {
            path: path.to_string_lossy().into_owned(),
            status: ImportStatus::Skipped,
            reason: Some(reason.into()),
            source: None,
        }
    }

    fn imported(path: &Path, source: CourseSource) -> Self {
        Self {
            path: path.to_string_lossy().into_owned(),
            status: ImportStatus::Imported,
            reason: None,
            source: Some(source),
        }
    }
}

pub(crate) fn list_all(root: &Path) -> Result<Vec<CourseSource>> {
    let dir = root.join("sources");
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(io_err(&dir, source)),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| io_err(&dir, source))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_visible_source_entry(&name) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let origin = read_origin(&path);
        let (kind, url, fetched_at) = match origin {
            Some(o) => (SourceKind::Url, Some(o.url), Some(o.fetched_at)),
            None => (classify_extension(&path), None, None),
        };
        out.push(CourseSource {
            name,
            path: path.to_string_lossy().into_owned(),
            kind,
            url,
            fetched_at,
            bytes,
        });
    }
    out.sort_by(|a, b| (stem_of(&a.name), &a.name).cmp(&(stem_of(&b.name), &b.name)));
    Ok(out)
}

pub(crate) fn import_files<P: AsRef<Path>>(root: &Path, paths: &[P]) -> Result<Vec<ImportResult>> {
    let sources_dir = root.join("sources");
    fs::create_dir_all(&sources_dir).map_err(|source| io_err(&sources_dir, source))?;

    let mut state = ImportState::load(&sources_dir)?;
    let mut results = Vec::new();

    for raw in paths {
        let path = raw.as_ref();
        if !path.is_absolute() {
            results.push(ImportResult::skipped(path, "path must be absolute"));
            continue;
        }
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) => {
                results.push(ImportResult::skipped(
                    path,
                    format!("cannot read file: {e}"),
                ));
                continue;
            }
        };
        let ft = meta.file_type();
        if ft.is_symlink() {
            results.push(ImportResult::skipped(path, "symlink is not followed"));
        } else if ft.is_file() {
            results.push(state.import_one(&sources_dir, path)?);
        } else if ft.is_dir() {
            let canonical_root = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            let mut folder_count = 0usize;
            state.walk_folder(
                &sources_dir,
                path,
                &canonical_root,
                0,
                &mut folder_count,
                &mut results,
            )?;
        } else {
            results.push(ImportResult::skipped(path, "unsupported file type"));
        }
    }

    Ok(results)
}

struct ImportState {
    taken_names: HashSet<String>,
    seen_bytes: Vec<Vec<u8>>,
}

impl ImportState {
    fn load(sources_dir: &Path) -> Result<Self> {
        let mut taken_names = HashSet::new();
        let mut seen_bytes = Vec::new();
        for entry in fs::read_dir(sources_dir).map_err(|source| io_err(sources_dir, source))? {
            let entry = entry.map_err(|source| io_err(sources_dir, source))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_visible_source_entry(&name) {
                continue;
            }
            if entry.file_type().is_ok_and(|t| t.is_file()) {
                taken_names.insert(name);
                if let Ok(bytes) = fs::read(entry.path()) {
                    seen_bytes.push(bytes);
                }
            }
        }
        Ok(Self {
            taken_names,
            seen_bytes,
        })
    }

    fn walk_folder(
        &mut self,
        sources_dir: &Path,
        dir: &Path,
        canonical_root: &Path,
        depth: usize,
        count: &mut usize,
        results: &mut Vec<ImportResult>,
    ) -> Result<()> {
        if *count >= MAX_IMPORT_FILES {
            return Ok(());
        }
        let read_dir = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                results.push(ImportResult::skipped(
                    dir,
                    format!("cannot read folder: {e}"),
                ));
                return Ok(());
            }
        };
        let mut entries: Vec<fs::DirEntry> = read_dir.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            if *count >= MAX_IMPORT_FILES {
                break;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_symlink() {
                let inside = fs::canonicalize(&path)
                    .map(|target| target.starts_with(canonical_root) && target.is_file())
                    .unwrap_or(false);
                if !inside {
                    results.push(ImportResult::skipped(
                        &path,
                        "symlink points outside the chosen folder",
                    ));
                    continue;
                }
                *count += 1;
                results.push(self.import_one(sources_dir, &path)?);
            } else if ft.is_file() {
                *count += 1;
                results.push(self.import_one(sources_dir, &path)?);
            } else if ft.is_dir() && depth < MAX_IMPORT_DEPTH {
                self.walk_folder(
                    sources_dir,
                    &path,
                    canonical_root,
                    depth + 1,
                    count,
                    results,
                )?;
            }
        }
        Ok(())
    }

    fn import_one(&mut self, sources_dir: &Path, path: &Path) -> Result<ImportResult> {
        let Some(kind) = supported_import_kind(path) else {
            return Ok(ImportResult::skipped(path, "unsupported file type"));
        };
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                return Ok(ImportResult::skipped(
                    path,
                    format!("cannot read file: {e}"),
                ))
            }
        };
        if self.seen_bytes.iter().any(|existing| existing == &bytes) {
            return Ok(ImportResult::skipped(path, "duplicate"));
        }
        let name = self.allocate_unique_name(path);
        let dest = sources_dir.join(&name);
        fs::write(&dest, &bytes).map_err(|source| io_err(&dest, source))?;
        let byte_len = bytes.len() as u64;
        self.taken_names.insert(name.clone());
        self.seen_bytes.push(bytes);

        Ok(ImportResult::imported(
            path,
            CourseSource {
                name,
                path: dest.to_string_lossy().into_owned(),
                kind,
                url: None,
                fetched_at: None,
                bytes: byte_len,
            },
        ))
    }

    fn allocate_unique_name(&self, path: &Path) -> String {
        let raw_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "source.txt".to_string());
        let (raw_stem, ext) = match raw_name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() && !e.is_empty() => (s, e.to_ascii_lowercase()),
            _ => (raw_name.as_str(), "txt".to_string()),
        };
        let stem = sanitize_stem(raw_stem);
        let mut candidate = format!("{stem}.{ext}");
        let mut copy = 2u32;
        while self.taken_names.contains(&candidate) {
            candidate = format!("{stem}-{copy}.{ext}");
            copy += 1;
        }
        candidate
    }
}

fn sanitize_stem(stem: &str) -> String {
    let trimmed = stem.trim_start_matches('.');
    let cleaned: String = trimmed
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == '\0' {
                '-'
            } else {
                c
            }
        })
        .collect();
    if cleaned.trim().is_empty() {
        "source".to_string()
    } else {
        cleaned
    }
}

fn is_visible_source_entry(name: &str) -> bool {
    !name.starts_with('.') && !name.ends_with(ORIGIN_SUFFIX)
}

fn supported_import_kind(path: &Path) -> Option<SourceKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => Some(SourceKind::Pdf),
        "md" | "markdown" => Some(SourceKind::Markdown),
        "txt" => Some(SourceKind::Text),
        _ => None,
    }
}

fn classify_extension(path: &Path) -> SourceKind {
    supported_import_kind(path).unwrap_or(SourceKind::Other)
}
