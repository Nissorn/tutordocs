//! Course setup and built-in course templates.

use serde::{Deserialize, Serialize};

use crate::error::{Result, StoreError};

const INTERVIEW_MD: &str = include_str!("../templates/interview.md");
const EXAM_MD: &str = include_str!("../templates/exam.md");
const OVERVIEW_MD: &str = include_str!("../templates/overview.md");
const CUSTOM_MD: &str = include_str!("../templates/custom.md");

const TEMPLATE_FILES: [(&str, &str); 4] = [
    ("interview", INTERVIEW_MD),
    ("exam", EXAM_MD),
    ("overview", OVERVIEW_MD),
    ("custom", CUSTOM_MD),
];

const VALID_LEVELS: [&str; 3] = ["new", "some", "strong"];
const VALID_LANGUAGES: [&str; 2] = ["th", "en"];
const MAX_HOURS_PER_WEEK: u32 = 168;

/// A course template as listed to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: String,
    pub title: String,
    pub description: String,
}

/// The learner's setup and template choice stored in `course.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseSetup {
    pub template: String,
    pub level: String,
    pub hours_per_week: Option<u32>,
    pub language: String,
    #[serde(default)]
    pub guide: String,
}

struct ParsedTemplate {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    instructions: &'static str,
}

fn parse_template(id: &'static str, raw: &'static str) -> ParsedTemplate {
    let (head, instructions) = raw.split_once("\n## Instructions").unwrap_or((raw, ""));
    let mut lines = head.lines();
    let title = lines
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .unwrap_or(id);
    let rest_start = head.find('\n').map(|idx| &head[idx + 1..]).unwrap_or("");
    let description = rest_start.trim();
    ParsedTemplate {
        id,
        title,
        description,
        instructions: instructions.trim(),
    }
}

/// List the built-in course templates in contract order.
pub fn list_course_templates() -> Vec<Template> {
    TEMPLATE_FILES
        .into_iter()
        .map(|(id, raw)| {
            let parsed = parse_template(id, raw);
            Template {
                id: parsed.id.to_string(),
                title: parsed.title.to_string(),
                description: parsed.description.to_string(),
            }
        })
        .collect()
}

/// The outline-prompt instructions carried by template `id`, if known.
pub fn template_instructions_for_id(id: &str) -> Option<&'static str> {
    TEMPLATE_FILES
        .into_iter()
        .find(|(tid, _)| *tid == id)
        .map(|(tid, raw)| parse_template(tid, raw).instructions)
}

impl CourseSetup {
    /// Validate external input against the contract's allowed values.
    pub fn validate(&self) -> Result<()> {
        if template_instructions_for_id(&self.template).is_none() {
            return Err(StoreError::Rejected(format!(
                "unknown course template {:?}",
                self.template
            )));
        }
        if !VALID_LEVELS.contains(&self.level.as_str()) {
            return Err(StoreError::Rejected(format!(
                "invalid course level {:?}: expected \"new\", \"some\" or \"strong\"",
                self.level
            )));
        }
        if !VALID_LANGUAGES.contains(&self.language.as_str()) {
            return Err(StoreError::Rejected(format!(
                "unsupported course language {:?}: expected \"th\" or \"en\"",
                self.language
            )));
        }
        if let Some(hours) = self.hours_per_week {
            if hours == 0 || hours > MAX_HOURS_PER_WEEK {
                return Err(StoreError::Rejected(format!(
                    "invalid hoursPerWeek {hours}: expected 1..={MAX_HOURS_PER_WEEK}"
                )));
            }
        }
        Ok(())
    }
}
