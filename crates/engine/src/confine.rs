//! A job folder under the library's staging directory.
//!
//! `JobSpec` can name one workdir and a tool allowlist. It has no field for a
//! directory the job may read but not write, and `--add-dir` would grant that
//! directory the same write permission as the workdir. Callers copy the files
//! a job may read into this folder and point the workdir at it.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tutordocs_store::StoreError;

use crate::generation::GenerationError;

/// The agent's workdir. Dropping it deletes the folder and whatever is left
/// in it, so a failed job cannot leave a half-written file behind.
pub(crate) struct JobFolder(PathBuf);

impl JobFolder {
    /// An empty directory `staging/<prefix>-<pid>-<n>`. A leftover from a
    /// crashed run with the same name is removed first.
    pub(crate) fn create(staging: &Path, prefix: &str) -> Result<Self, GenerationError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = staging.join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).map_err(|source| io_error(&path, source))?;
        Ok(Self(path))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for JobFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Copy each named file from `root/sources/` into `dest`. An empty list copies
/// nothing and does not create `dest`. A name that is not one path component
/// is refused, so the copy cannot climb out of `dest`.
pub(crate) fn copy_named_sources(
    root: &Path,
    dest: &Path,
    names: &[String],
) -> Result<(), GenerationError> {
    if names.is_empty() {
        return Ok(());
    }
    fs::create_dir(dest).map_err(|source| io_error(dest, source))?;
    for name in names {
        if !is_plain_name(name) {
            return Err(GenerationError::InvalidRequest(format!(
                "source name {name:?} is not a single file name"
            )));
        }
        let from = root.join("sources").join(name);
        let to = dest.join(name);
        fs::copy(&from, &to).map_err(|source| io_error(&from, source))?;
    }
    Ok(())
}

/// Copy every regular file in `root/sources/` into `dest`. An empty directory
/// copies nothing and does not create `dest`. Symlinks are skipped, so a link
/// cannot point the snapshot at a path outside `sources/`.
pub(crate) fn copy_source_files(root: &Path, dest: &Path) -> Result<(), GenerationError> {
    let from = root.join("sources");
    let mut created = false;
    let entries = fs::read_dir(&from).map_err(|source| io_error(&from, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| io_error(&from, source))?;
        let is_file = entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false);
        if !is_file {
            continue;
        }
        if !created {
            fs::create_dir(dest).map_err(|source| io_error(dest, source))?;
            created = true;
        }
        let to = dest.join(entry.file_name());
        fs::copy(entry.path(), &to).map_err(|source| io_error(&to, source))?;
    }
    Ok(())
}

/// Copy `course.json` into `dest_dir` under the same name.
pub(crate) fn copy_course(root: &Path, dest_dir: &Path) -> Result<(), GenerationError> {
    let from = root.join("course.json");
    let to = dest_dir.join("course.json");
    fs::copy(&from, &to).map_err(|source| io_error(&from, source))?;
    Ok(())
}

pub(crate) fn is_plain_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    )
}

fn io_error(path: &Path, source: std::io::Error) -> GenerationError {
    StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
    .into()
}
