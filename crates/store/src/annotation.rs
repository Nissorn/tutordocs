//! Annotations: an anchor plus a payload, stored in SQLite, never derived from
//! the markdown and never rewriting it.
//!
//! Field shape and selector normalization are translated from DeepTutor
//! `reading/models.py` (Apache-2.0, (c) 2025 Data Intelligence Lab, HKU), pinned
//! commit 60fb3bdf: `Annotation` (:358-445), `TextQuoteSelector` (:291-306),
//! `TextPositionSelector` (:309-318) and `parse_text_selectors` (:324-355).
//! `material_id` is the owning document key, which DeepTutor's storage layer
//! also carries alongside the annotation. The `rects` geometry is passed
//! through as opaque JSON; its shape belongs to the PDF/reader tickets.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// The payload of a mark. The ticket names three: a highlight color, a margin
/// note, or a bookmark. DeepTutor's `underline`/`citation` kinds are not ported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AnnotationKind {
    /// A colored highlight over the quoted text.
    #[default]
    Highlight,
    /// A margin note attached to the anchor.
    Note,
    /// A bookmark with no color or note body.
    Bookmark,
}

impl AnnotationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Highlight => "highlight",
            Self::Note => "note",
            Self::Bookmark => "bookmark",
        }
    }

    /// Unknown kinds fall back to highlight, mirroring `models.py:427`.
    fn parse(value: &str) -> Self {
        match value {
            "note" => Self::Note,
            "bookmark" => Self::Bookmark,
            _ => Self::Highlight,
        }
    }
}

/// The five-color palette kept server-side in DeepTutor (`models.py:38-46`).
const COLORS: [&str; 5] = ["yellow", "green", "blue", "pink", "purple"];
/// Default color (`models.py:46`).
pub const DEFAULT_COLOR: &str = "yellow";
/// Maximum exact-quote length (`MAX_TEXT_SELECTOR_CHARS`, `models.py:33`).
const MAX_EXACT_CHARS: usize = 2000;
/// Prefix/suffix context cap (`models.py:343-344`).
const MAX_CONTEXT_CHARS: usize = 128;

fn normalize_color(value: &str) -> String {
    if COLORS.contains(&value) {
        value.to_string()
    } else {
        DEFAULT_COLOR.to_string()
    }
}

/// A W3C text selector. The quote selector re-anchors after reflow; the
/// position selector is the fallback in a rendered unit's text-content space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TextSelector {
    /// W3C TextQuoteSelector (`models.py:291-306`).
    TextQuoteSelector {
        /// The exact quoted text the mark covers.
        exact: String,
        /// Text immediately before `exact` (tail kept on normalize).
        #[serde(default)]
        prefix: String,
        /// Text immediately after `exact` (head kept on normalize).
        #[serde(default)]
        suffix: String,
    },
    /// W3C TextPositionSelector (`models.py:309-318`).
    TextPositionSelector {
        /// Start offset, clamped to >= 0.
        start: i64,
        /// End offset, must be > start.
        end: i64,
    },
}

/// Apply the load-time invariants of `parse_text_selectors` (`models.py:324-355`):
/// at most two selectors, exact <= 2000 chars (empty dropped), prefix keeps its
/// last 128 chars, suffix keeps its first 128, position start >= 0 and end > start.
pub fn normalize_selectors(selectors: Vec<TextSelector>) -> Vec<TextSelector> {
    let mut out = Vec::new();
    for selector in selectors.into_iter().take(2) {
        match selector {
            TextSelector::TextQuoteSelector {
                exact,
                prefix,
                suffix,
            } => {
                let exact: String = exact.chars().take(MAX_EXACT_CHARS).collect();
                if exact.is_empty() {
                    continue;
                }
                let prefix: String = tail_chars(&prefix, MAX_CONTEXT_CHARS);
                let suffix: String = suffix.chars().take(MAX_CONTEXT_CHARS).collect();
                out.push(TextSelector::TextQuoteSelector {
                    exact,
                    prefix,
                    suffix,
                });
            }
            TextSelector::TextPositionSelector { start, end } => {
                let start = start.max(0);
                let end = end.max(start);
                if end > start {
                    out.push(TextSelector::TextPositionSelector { start, end });
                }
            }
        }
    }
    out
}

fn tail_chars(value: &str, n: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    let start = chars.len().saturating_sub(n);
    chars[start..].iter().collect()
}

/// One annotation. Fields mirror DeepTutor `Annotation` (`models.py:358-445`);
/// `material_id` is the document (chapter or source) the mark lives on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    /// Stable identifier (`annotation_id`).
    pub annotation_id: String,
    /// Owning chapter/source id; the storage key DeepTutor keeps separately.
    pub material_id: String,
    /// Unit locator within the material, clamped to >= 1 on read.
    pub locator: i64,
    /// Content revision the anchor was captured against, clamped to >= 1.
    pub material_revision: i64,
    /// Payload kind.
    pub kind: AnnotationKind,
    /// Highlight color from the palette.
    pub color: String,
    /// The quoted text the mark covers; the portable anchor.
    pub quote: String,
    /// Margin note body.
    pub note: String,
    /// Optional renderer geometry, opaque JSON (PDF/reader tickets own it).
    pub rects: Vec<serde_json::Value>,
    /// Opaque renderer-native anchor (e.g. an EPUB CFI).
    pub source_anchor: String,
    /// Portable W3C selectors, quote first with position fallback.
    pub selectors: Vec<TextSelector>,
    /// resolved | unresolved | ambiguous; unknown falls back to resolved.
    pub resolution: String,
    /// Who made the mark; defaults to "user".
    pub author: String,
    /// Unix seconds.
    pub created_at: f64,
    /// Unix seconds.
    pub updated_at: f64,
}

impl Annotation {
    /// A highlight with sensible defaults, mirroring the dataclass defaults.
    pub fn new(annotation_id: impl Into<String>, material_id: impl Into<String>) -> Self {
        let now = unix_now();
        Self {
            annotation_id: annotation_id.into(),
            material_id: material_id.into(),
            locator: 1,
            material_revision: 1,
            kind: AnnotationKind::Highlight,
            color: DEFAULT_COLOR.to_string(),
            quote: String::new(),
            note: String::new(),
            rects: Vec::new(),
            source_anchor: String::new(),
            selectors: Vec::new(),
            resolution: "resolved".to_string(),
            author: "user".to_string(),
            created_at: now,
            updated_at: now,
        }
    }
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn normalize_resolution(value: &str) -> String {
    match value {
        "unresolved" | "ambiguous" => value.to_string(),
        _ => "resolved".to_string(),
    }
}

/// Insert or replace an annotation by id. Selectors and palette are normalized
/// on the way in so stored rows always satisfy the load-time invariants.
pub(crate) fn put(conn: &Connection, a: &Annotation) -> Result<()> {
    let selectors = normalize_selectors(a.selectors.clone());
    let selectors_json = serde_json::to_string(&selectors)?;
    let rects_json = serde_json::to_string(&a.rects)?;
    conn.execute(
        "INSERT INTO annotations (
            annotation_id, material_id, locator, material_revision, kind, color,
            quote, note, rects_json, source_anchor, selectors_json, resolution,
            author, created_at, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
         ON CONFLICT(annotation_id) DO UPDATE SET
            material_id=excluded.material_id,
            locator=excluded.locator,
            material_revision=excluded.material_revision,
            kind=excluded.kind,
            color=excluded.color,
            quote=excluded.quote,
            note=excluded.note,
            rects_json=excluded.rects_json,
            source_anchor=excluded.source_anchor,
            selectors_json=excluded.selectors_json,
            resolution=excluded.resolution,
            author=excluded.author,
            updated_at=excluded.updated_at",
        params![
            a.annotation_id,
            a.material_id,
            a.locator.max(1),
            a.material_revision.max(1),
            a.kind.as_str(),
            normalize_color(&a.color),
            a.quote,
            a.note,
            rects_json,
            a.source_anchor,
            selectors_json,
            normalize_resolution(&a.resolution),
            if a.author.is_empty() {
                "user"
            } else {
                &a.author
            },
            a.created_at,
            a.updated_at,
        ],
    )?;
    Ok(())
}

const SELECT_COLS: &str = "annotation_id, material_id, locator, material_revision, \
    kind, color, quote, note, rects_json, source_anchor, selectors_json, \
    resolution, author, created_at, updated_at";

fn row_to_annotation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Annotation> {
    let rects_json: String = row.get(8)?;
    let selectors_json: String = row.get(10)?;
    let kind: String = row.get(4)?;
    let color: String = row.get(5)?;
    let resolution: String = row.get(11)?;
    let author: String = row.get(12)?;
    Ok(Annotation {
        annotation_id: row.get(0)?,
        material_id: row.get(1)?,
        locator: row.get::<_, i64>(2)?.max(1),
        material_revision: row.get::<_, i64>(3)?.max(1),
        kind: AnnotationKind::parse(&kind),
        color: normalize_color(&color),
        quote: row.get(6)?,
        note: row.get(7)?,
        rects: serde_json::from_str(&rects_json).unwrap_or_default(),
        source_anchor: row.get(9)?,
        selectors: serde_json::from_str(&selectors_json).unwrap_or_default(),
        resolution: normalize_resolution(&resolution),
        author: if author.is_empty() {
            "user".to_string()
        } else {
            author
        },
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

/// Read one annotation by id, applying the legacy-row clamps on the way out.
pub(crate) fn get(conn: &Connection, id: &str) -> Result<Option<Annotation>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM annotations WHERE annotation_id = ?1"
    ))?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_annotation(row)?)),
        None => Ok(None),
    }
}

/// List a material's annotations in locator order.
pub(crate) fn list_for(conn: &Connection, material_id: &str) -> Result<Vec<Annotation>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM annotations WHERE material_id = ?1 \
         ORDER BY locator, annotation_id"
    ))?;
    let rows = stmt.query_map(params![material_id], row_to_annotation)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Delete a mark by id. Returns whether a row was removed. This is an explicit
/// user action; regeneration never calls it (the truth rule).
pub(crate) fn delete(conn: &Connection, id: &str) -> Result<bool> {
    let changed = conn.execute(
        "DELETE FROM annotations WHERE annotation_id = ?1",
        params![id],
    )?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_normalization_caps_and_trims() {
        let long_prefix = "x".repeat(300);
        let long_suffix = "y".repeat(300);
        let selectors = vec![
            TextSelector::TextQuoteSelector {
                exact: "q".repeat(2500),
                prefix: long_prefix,
                suffix: long_suffix,
            },
            TextSelector::TextPositionSelector { start: 3, end: 9 },
            TextSelector::TextPositionSelector {
                start: 100,
                end: 200,
            },
        ];
        let out = normalize_selectors(selectors);
        assert_eq!(out.len(), 2, "only the first two input entries are kept");
        match &out[0] {
            TextSelector::TextQuoteSelector {
                exact,
                prefix,
                suffix,
            } => {
                assert_eq!(exact.chars().count(), 2000);
                assert_eq!(prefix.chars().count(), 128);
                assert_eq!(suffix.chars().count(), 128);
            }
            _ => panic!("first selector must stay a quote"),
        }
        assert_eq!(
            out[1],
            TextSelector::TextPositionSelector { start: 3, end: 9 },
            "the third entry is beyond the two-selector cap"
        );
    }

    #[test]
    fn degenerate_position_is_dropped_after_slicing() {
        // parse_text_selectors slices the first two entries, then filters: a
        // degenerate position among them is dropped, a later valid one is not
        // promoted into its place.
        let out = normalize_selectors(vec![
            TextSelector::TextPositionSelector { start: -5, end: -1 },
            TextSelector::TextPositionSelector { start: 3, end: 9 },
        ]);
        assert_eq!(
            out,
            vec![TextSelector::TextPositionSelector { start: 3, end: 9 }]
        );
        let equal = normalize_selectors(vec![TextSelector::TextPositionSelector {
            start: 5,
            end: 5,
        }]);
        assert!(equal.is_empty(), "end must strictly exceed start");
    }

    #[test]
    fn empty_exact_is_dropped() {
        let out = normalize_selectors(vec![TextSelector::TextQuoteSelector {
            exact: String::new(),
            prefix: String::new(),
            suffix: String::new(),
        }]);
        assert!(out.is_empty());
    }

    #[test]
    fn unknown_color_and_kind_fall_back() {
        assert_eq!(normalize_color("chartreuse"), "yellow");
        assert_eq!(
            AnnotationKind::parse("underline"),
            AnnotationKind::Highlight
        );
        assert_eq!(normalize_resolution("bogus"), "resolved");
    }
}
