//! SQLite connection bootstrap and schema.
//!
//! The `mastery_events` DDL is translated from DeepTutor `learning/storage.py`
//! (Apache-2.0, (c) 2025 Data Intelligence Lab, HKU), pinned commit
//! 60fb3bdf. The eight columns and the `(path_id, revision, id)` index are
//! preserved. The original `REFERENCES mastery_paths(path_id)` clause is
//! dropped on purpose: TutorDocs derives repetition state from the log instead
//! of storing a mutable `mastery_paths` snapshot, so there is no parent table.
//!
//! Decision 10 is binding: no FTS, no embeddings, no vector table, no sync
//! machinery. The only index here is the ported event-log index.

use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use crate::error::Result;

/// Schema version. Bump only when a migration is added; the bootstrap is
/// idempotent (`CREATE ... IF NOT EXISTS`) so reopening never rewrites rows.
const USER_VERSION: i32 = 1;

/// Open (or create) the store database and apply the schema. WAL is set first,
/// mirroring DeepTutor `storage.py:529`, so a reader never blocks the writer.
/// Every connection waits five seconds on a lock before `SQLITE_BUSY`, so a
/// second writer in the same grade is not failed at once.
pub(crate) fn open_db(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS annotations (
            annotation_id     TEXT PRIMARY KEY,
            material_id       TEXT NOT NULL,
            locator           INTEGER NOT NULL,
            material_revision INTEGER NOT NULL DEFAULT 1,
            kind              TEXT NOT NULL DEFAULT 'highlight',
            color             TEXT NOT NULL DEFAULT 'yellow',
            quote             TEXT NOT NULL DEFAULT '',
            note              TEXT NOT NULL DEFAULT '',
            rects_json        TEXT NOT NULL DEFAULT '[]',
            source_anchor     TEXT NOT NULL DEFAULT '',
            selectors_json    TEXT NOT NULL DEFAULT '[]',
            resolution        TEXT NOT NULL DEFAULT 'resolved',
            author            TEXT NOT NULL DEFAULT 'user',
            created_at        REAL NOT NULL,
            updated_at        REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_annotations_material
            ON annotations(material_id, locator);

        CREATE TABLE IF NOT EXISTS mastery_events (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            path_id      TEXT NOT NULL,
            revision     INTEGER NOT NULL,
            event_type   TEXT NOT NULL,
            payload_json TEXT NOT NULL DEFAULT '{}',
            session_id   TEXT NOT NULL DEFAULT '',
            turn_id      TEXT NOT NULL DEFAULT '',
            created_at   REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_mastery_events_path_revision
            ON mastery_events(path_id, revision, id);

        CREATE TABLE IF NOT EXISTS answers (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            chapter_id   TEXT NOT NULL,
            quote        TEXT,
            question     TEXT NOT NULL,
            answer       TEXT NOT NULL,
            sources_json TEXT NOT NULL DEFAULT '[]',
            asked_at     TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_answers_chapter
            ON answers(chapter_id, id);
        ",
    )?;
    conn.pragma_update(None, "user_version", USER_VERSION)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_db_sets_a_busy_timeout_on_every_connection() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(&dir.path().join("store.db")).unwrap();
        let timeout_ms: i64 = conn
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .unwrap();
        assert_eq!(timeout_ms, 5_000);
    }
}
