//! Saved answers to questions asked about a chapter or passage.
//!
//! Stored in the course's SQLite database per chapter, never in the markdown
//! files (PROPOSAL truth rule).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// One answer before it is inserted into SQLite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAnswer {
    pub quote: Option<String>,
    pub question: String,
    pub answer: String,
    pub sources: Vec<String>,
    pub asked_at: String,
}

/// One stored answer for a chapter, matching the `Answer` wire shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterAnswer {
    pub id: i64,
    pub quote: Option<String>,
    pub question: String,
    pub answer: String,
    pub sources: Vec<String>,
    pub asked_at: String,
}

/// Insert one answer for `chapter_id` and return the stored record with its id.
pub(crate) fn save(
    conn: &Connection,
    chapter_id: &str,
    entry: &NewAnswer,
) -> Result<ChapterAnswer> {
    let sources_json = serde_json::to_string(&entry.sources)?;
    conn.execute(
        "INSERT INTO answers (chapter_id, quote, question, answer, sources_json, asked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            chapter_id,
            entry.quote,
            entry.question,
            entry.answer,
            sources_json,
            entry.asked_at,
        ],
    )?;
    let id = conn.last_insert_rowid();
    Ok(ChapterAnswer {
        id,
        quote: entry.quote.clone(),
        question: entry.question.clone(),
        answer: entry.answer.clone(),
        sources: entry.sources.clone(),
        asked_at: entry.asked_at.clone(),
    })
}

/// List a chapter's answers, newest first.
pub(crate) fn list_for(conn: &Connection, chapter_id: &str) -> Result<Vec<ChapterAnswer>> {
    let mut stmt = conn.prepare(
        "SELECT id, quote, question, answer, sources_json, asked_at
         FROM answers
         WHERE chapter_id = ?1
         ORDER BY asked_at DESC, id DESC",
    )?;
    let rows = stmt.query_map(params![chapter_id], |row| {
        let sources_json: String = row.get(4)?;
        Ok(ChapterAnswer {
            id: row.get(0)?,
            quote: row.get(1)?,
            question: row.get(2)?,
            answer: row.get(3)?,
            sources: serde_json::from_str(&sources_json).unwrap_or_default(),
            asked_at: row.get(5)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
